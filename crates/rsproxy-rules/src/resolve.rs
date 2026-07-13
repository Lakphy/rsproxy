use super::*;

impl RuleSet {
    /// Creates an indexed snapshot with no rules and a fresh publication version.
    pub fn empty() -> Self {
        Self {
            version: now_millis(),
            rules: Vec::new(),
            index: RuleIndex::default(),
        }
    }

    /// Parses one named DSL group, returning every source-located error found.
    ///
    /// No partially valid snapshot is published when any non-empty line fails.
    pub fn parse(group: &str, text: &str) -> Result<Self, Vec<RuleError>> {
        Self::parse_groups([(group, text)])
    }

    /// Parses groups in iteration order and compiles one immutable candidate index.
    ///
    /// Group order precedes line order during resolution. All diagnostics across
    /// all groups are accumulated before this returns `Err`.
    pub fn parse_groups<I, G, T>(groups: I) -> Result<Self, Vec<RuleError>>
    where
        I: IntoIterator<Item = (G, T)>,
        G: AsRef<str>,
        T: AsRef<str>,
    {
        let mut rules = Vec::new();
        let mut errors = Vec::new();

        for (group, text) in groups {
            let group = group.as_ref();
            for (idx, raw_line) in text.as_ref().lines().enumerate() {
                let line_no = idx + 1;
                let Some(line) = strip_comment(raw_line) else {
                    continue;
                };
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                match parse_rule(group, line_no, line) {
                    Ok(rule) => rules.push(rule),
                    Err(error) => errors.push(RuleError {
                        code: error.code,
                        group: group.to_string(),
                        line: line_no,
                        message: error.source.to_string(),
                    }),
                }
            }
        }

        if errors.is_empty() {
            let index = RuleIndex::build(&rules);
            Ok(Self {
                version: now_millis(),
                rules,
                index,
            })
        } else {
            Err(errors)
        }
    }

    /// Resolves request-phase actions with request-body-dependent rules enabled.
    ///
    /// Response-only conditions do not match because no response snapshot exists.
    pub fn resolve(&self, req: &RequestMeta) -> ResolveResult {
        self.resolve_inner(req, None, true)
    }

    /// Resolves with an immutable response snapshot available to conditions and templates.
    pub fn resolve_response(&self, req: &RequestMeta, res: &ResponseMeta) -> ResolveResult {
        self.resolve_inner(req, Some(res), true)
    }

    /// Resolves request-phase actions while omitting body-dependent conditions and actions.
    ///
    /// Non-body operations in a mixed [`DeleteOp`] action are retained.
    pub fn resolve_without_request_body(&self, req: &RequestMeta) -> ResolveResult {
        self.resolve_inner(req, None, false)
    }

    /// Resolves response-phase actions without evaluating or returning request-body work.
    pub fn resolve_response_without_request_body(
        &self,
        req: &RequestMeta,
        res: &ResponseMeta,
    ) -> ResolveResult {
        self.resolve_inner(req, Some(res), false)
    }

    /// Reports whether any still-viable rule needs the request body to decide or execute.
    ///
    /// Matcher and body-independent conditions are evaluated first, avoiding body
    /// buffering when every candidate has already been ruled out.
    pub fn request_body_required(&self, req: &RequestMeta) -> bool {
        let parts = UrlParts::parse(&req.url);
        self.candidate_rule_indices(parts.as_ref().ok(), &req.url)
            .into_iter()
            .map(|idx| &self.rules[idx])
            .filter(|rule| !rule.disabled)
            .filter(|rule| {
                parts
                    .as_ref()
                    .ok()
                    .and_then(|url| rule.matcher.matches(url, &req.url))
                    .is_some()
            })
            .filter(|rule| {
                rule.conditions.iter().all(|condition| {
                    condition.may_match_before_request_body(req, parts.as_ref().ok(), rule.line)
                })
            })
            .any(|rule| {
                rule.conditions
                    .iter()
                    .any(Condition::depends_on_request_body)
                    || rule.actions.iter().any(action_requires_request_body)
            })
    }

    pub(super) fn resolve_inner(
        &self,
        req: &RequestMeta,
        res: Option<&ResponseMeta>,
        include_request_body: bool,
    ) -> ResolveResult {
        if self.rules.is_empty() {
            return ResolveResult {
                actions: Vec::new(),
                matched_rules: Vec::new(),
            };
        }
        let parts = UrlParts::parse(&req.url);
        let mut actions = Vec::new();
        let mut matched_rules = Vec::new();
        let mut seen_rules = HashSet::new();
        let mut single_families = HashSet::new();
        let mut skipped_families = HashSet::new();
        let mut skip_all = false;
        let response = res.cloned().map(Arc::new);

        let mut ordered: Vec<usize> = self.candidate_rule_indices(parts.as_ref().ok(), &req.url);
        ordered.sort_by_key(|idx| {
            let rule = &self.rules[*idx];
            (!rule.important, *idx)
        });

        for idx in ordered {
            let rule = &self.rules[idx];
            if rule.disabled {
                continue;
            }
            let captures = match parts
                .as_ref()
                .ok()
                .and_then(|url| rule.matcher.matches(url, &req.url))
            {
                Some(captures) => captures,
                None => continue,
            };

            if !include_request_body
                && rule
                    .conditions
                    .iter()
                    .any(Condition::depends_on_request_body)
            {
                continue;
            }

            if !rule
                .conditions
                .iter()
                .all(|condition| condition.matches(req, parts.as_ref().ok(), res, rule.line))
            {
                continue;
            }

            let brief = MatchedRule {
                group: rule.group.clone(),
                line: rule.line,
                raw: rule.raw.clone(),
            };
            for action in &rule.actions {
                let Some(action) = action_for_body_availability(action, include_request_body)
                else {
                    continue;
                };
                let family = action.family();
                if skip_all || family_is_skipped(family, &skipped_families) {
                    continue;
                }
                if action.is_single() && single_families.contains(family) {
                    continue;
                }
                if action.is_single() {
                    single_families.insert(family.to_string());
                }
                if seen_rules.insert((brief.group.clone(), brief.line)) {
                    matched_rules.push(brief.clone());
                }
                if let Action::Skip(families) = &action {
                    if families.is_empty()
                        || families.iter().any(|family| {
                            let family = normalize_skip_family(family);
                            family == "*" || family == "all"
                        })
                    {
                        skip_all = true;
                    } else {
                        skipped_families
                            .extend(families.iter().map(|family| normalize_skip_family(family)));
                    }
                }
                actions.push(ResolvedAction {
                    action,
                    rule: brief.clone(),
                    captures: captures.clone(),
                    response: response.clone(),
                });
            }
        }

        ResolveResult {
            actions,
            matched_rules,
        }
    }

    pub(super) fn candidate_rule_indices(
        &self,
        url: Option<&UrlParts>,
        raw_url: &str,
    ) -> Vec<usize> {
        let Some(url) = url else {
            return (0..self.rules.len()).collect();
        };
        let host = url.host.trim_matches(['[', ']']).to_ascii_lowercase();
        let mut indices = Vec::new();
        let mut seen = HashSet::new();

        if let Some(exact) = self.index.domain_exact.get(&host) {
            extend_unique(&mut indices, &mut seen, exact);
        }
        for suffix in host_suffixes(&host) {
            if let Some(bucket) = self.index.domain_suffix.get(suffix) {
                extend_unique(&mut indices, &mut seen, bucket);
            }
        }
        extend_unique(&mut indices, &mut seen, &self.index.global);
        let prefilter_matches = self.index.prefilter_matches(raw_url);
        extend_unique(&mut indices, &mut seen, &prefilter_matches);

        if indices.is_empty() && !raw_url.is_empty() {
            return Vec::new();
        }
        indices
    }

    /// Returns candidate-index counts for diagnostics and local resolver benchmarks.
    pub fn stats(&self) -> RuleSetStats {
        self.index.stats(&self.rules)
    }

    /// Formats request-phase resolution as stable human-readable rule provenance.
    pub fn explain(&self, req: &RequestMeta) -> String {
        explain_result(self.resolve(req), req)
    }

    /// Formats response-aware resolution with response templates available.
    pub fn explain_response(&self, req: &RequestMeta, res: &ResponseMeta) -> String {
        explain_result(self.resolve_response(req, res), req)
    }
}

fn action_requires_request_body(action: &Action) -> bool {
    matches!(action, Action::ReqBody(_))
        || matches!(
            action,
            Action::Delete(operations)
                if operations.iter().any(|operation| {
                    matches!(operation, DeleteOp::ReqBody | DeleteOp::ReqBodyPath(_))
                })
        )
}

fn action_for_body_availability(action: &Action, request_body_available: bool) -> Option<Action> {
    if request_body_available {
        return Some(action.clone());
    }
    match action {
        Action::ReqBody(_) => None,
        Action::Delete(operations) => {
            let operations = operations
                .iter()
                .filter(|operation| {
                    !matches!(operation, DeleteOp::ReqBody | DeleteOp::ReqBodyPath(_))
                })
                .cloned()
                .collect::<Vec<_>>();
            (!operations.is_empty()).then_some(Action::Delete(operations))
        }
        _ => Some(action.clone()),
    }
}

fn explain_result(result: ResolveResult, req: &RequestMeta) -> String {
    if result.actions.is_empty() {
        return "no matched actions".to_string();
    }

    let mut out = String::new();
    for item in &result.actions {
        out.push_str(&format!(
            "{}:{} {}\n",
            item.rule.group,
            item.rule.line,
            explain_action(item, req)
        ));
    }
    out
}

pub(super) fn family_is_skipped(family: &str, skipped_families: &HashSet<String>) -> bool {
    skipped_families
        .iter()
        .any(|skipped| family == skipped || family.starts_with(&format!("{skipped}.")))
}

pub(super) fn normalize_skip_family(family: &str) -> String {
    family
        .trim()
        .trim_matches(['"', '\''])
        .to_ascii_lowercase()
        .replace(['_', '-'], ".")
}
