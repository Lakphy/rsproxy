use super::*;

impl RuleSet {
    /// Creates an indexed snapshot with no rules and a fresh publication version.
    pub fn empty() -> Self {
        Self {
            language_version: RULE_LANGUAGE_VERSION,
            version: next_ruleset_version(),
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

    /// Parses a standalone v3 source whose first effective line is
    /// [`RULE_LANGUAGE_HEADER`]. Empty sources remain valid.
    ///
    /// Versioned sources accept canonical action and condition names only. Use
    /// [`migrate_rule_source_v3`] to upgrade an unversioned v2 source.
    pub fn parse_versioned(group: &str, text: &str) -> Result<Self, Vec<RuleError>> {
        Self::parse_versioned_groups([(group, text)])
    }

    /// Parses groups in iteration order and compiles one immutable candidate index.
    ///
    /// Group order precedes line order during resolution. Diagnostics are
    /// accumulated up to [`MAX_RULE_DIAGNOSTICS`]; the final entry reports when
    /// remaining source was intentionally not parsed.
    pub fn parse_groups<I, G, T>(groups: I) -> Result<Self, Vec<RuleError>>
    where
        I: IntoIterator<Item = (G, T)>,
        G: AsRef<str>,
        T: AsRef<str>,
    {
        Self::parse_groups_with_profile(groups, false)
    }

    /// Parses independently versioned v3 groups in iteration order.
    ///
    /// Every non-empty group must declare [`RULE_LANGUAGE_HEADER`] on its first
    /// effective (non-comment, non-blank) source line.
    pub fn parse_versioned_groups<I, G, T>(groups: I) -> Result<Self, Vec<RuleError>>
    where
        I: IntoIterator<Item = (G, T)>,
        G: AsRef<str>,
        T: AsRef<str>,
    {
        Self::parse_groups_with_profile(groups, true)
    }

    fn parse_groups_with_profile<I, G, T>(
        groups: I,
        require_language_header: bool,
    ) -> Result<Self, Vec<RuleError>>
    where
        I: IntoIterator<Item = (G, T)>,
        G: AsRef<str>,
        T: AsRef<str>,
    {
        let mut rules = Vec::new();
        let mut errors = Vec::new();
        let mut source_bytes = 0usize;
        let mut source_rule_lines = 0usize;
        let mut snapshot_actions = 0usize;
        let mut snapshot_condition_nodes = 0usize;
        let mut snapshot_body_conditions = 0usize;

        'groups: for (group_index, (group, text)) in groups.into_iter().enumerate() {
            let group = group.as_ref();
            let text = text.as_ref();
            let group_name_valid = !group.is_empty() && group.len() <= MAX_RULE_GROUP_NAME_BYTES;
            let diagnostic_group = if group_name_valid {
                group
            } else {
                "<invalid-group>"
            };

            if group_index >= MAX_RULE_GROUPS_PER_SNAPSHOT {
                push_parse_diagnostic(
                    &mut errors,
                    parse_error(
                        RuleErrorCode::Syntax,
                        diagnostic_group,
                        1,
                        format!("snapshot exceeds the {MAX_RULE_GROUPS_PER_SNAPSHOT}-group limit"),
                    ),
                );
                break;
            }
            source_bytes = match source_bytes.checked_add(text.len()) {
                Some(bytes) if bytes <= MAX_RULE_SNAPSHOT_SOURCE_BYTES => bytes,
                _ => {
                    push_parse_diagnostic(
                        &mut errors,
                        parse_error(
                            RuleErrorCode::Syntax,
                            diagnostic_group,
                            1,
                            format!(
                                "snapshot source exceeds the {MAX_RULE_SNAPSHOT_SOURCE_BYTES}-byte limit"
                            ),
                        ),
                    );
                    break;
                }
            };
            if !group_name_valid {
                if push_parse_diagnostic(
                    &mut errors,
                    parse_error(
                        RuleErrorCode::Syntax,
                        diagnostic_group,
                        1,
                        format!(
                            "rule group name must contain 1..={MAX_RULE_GROUP_NAME_BYTES} bytes"
                        ),
                    ),
                ) {
                    break;
                }
                continue;
            }

            let mut first_effective_line_seen = false;
            let mut syntax_profile = SyntaxProfile::Compatible;
            for (idx, raw_line) in text.lines().enumerate() {
                let line_no = idx + 1;
                if raw_line.len() > MAX_RULE_LINE_BYTES {
                    if push_parse_diagnostic(
                        &mut errors,
                        parse_error(
                            RuleErrorCode::Syntax,
                            group,
                            line_no,
                            format!("rule line exceeds the {MAX_RULE_LINE_BYTES}-byte limit"),
                        ),
                    ) {
                        break 'groups;
                    }
                    continue;
                }
                let Some(line) = strip_comment(raw_line) else {
                    continue;
                };
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if !first_effective_line_seen {
                    first_effective_line_seen = true;
                    if is_language_directive(line) {
                        if line != RULE_LANGUAGE_HEADER {
                            push_parse_diagnostic(
                                &mut errors,
                                parse_error(
                                    RuleErrorCode::Property,
                                    group,
                                    line_no,
                                    format!(
                                        "unsupported rule language directive `{line}`; expected `{RULE_LANGUAGE_HEADER}`"
                                    ),
                                ),
                            );
                            break;
                        }
                        syntax_profile = SyntaxProfile::CanonicalV3;
                        continue;
                    }
                    if require_language_header {
                        push_parse_diagnostic(
                            &mut errors,
                            parse_error(
                                RuleErrorCode::Property,
                                group,
                                line_no,
                                format!(
                                    "missing `{RULE_LANGUAGE_HEADER}`; it must be the first effective source line"
                                ),
                            ),
                        );
                        break;
                    }
                } else if is_language_directive(line) {
                    if push_parse_diagnostic(
                        &mut errors,
                        parse_error(
                            RuleErrorCode::Property,
                            group,
                            line_no,
                            format!(
                                "`{RULE_LANGUAGE_HEADER}` may appear only on the first effective source line"
                            ),
                        ),
                    ) {
                        break 'groups;
                    }
                    continue;
                }
                if source_rule_lines == MAX_RULES_PER_SNAPSHOT {
                    push_parse_diagnostic(
                        &mut errors,
                        parse_error(
                            RuleErrorCode::Syntax,
                            group,
                            line_no,
                            format!("snapshot exceeds the {MAX_RULES_PER_SNAPSHOT}-rule limit"),
                        ),
                    );
                    break 'groups;
                }
                source_rule_lines += 1;

                match parse_rule(group, line_no, line, syntax_profile) {
                    Ok(rule) => {
                        let condition_nodes =
                            rule.conditions.iter().map(condition_node_count).sum();
                        let body_conditions =
                            rule.conditions.iter().map(body_condition_count).sum();
                        let over_budget = if charge(
                            &mut snapshot_actions,
                            rule.actions.len(),
                            MAX_RULE_ACTIONS_PER_SNAPSHOT,
                        ) {
                            Some((
                                RuleErrorCode::Action,
                                MAX_RULE_ACTIONS_PER_SNAPSHOT,
                                "action",
                            ))
                        } else if charge(
                            &mut snapshot_condition_nodes,
                            condition_nodes,
                            MAX_RULE_CONDITION_NODES_PER_SNAPSHOT,
                        ) {
                            Some((
                                RuleErrorCode::Condition,
                                MAX_RULE_CONDITION_NODES_PER_SNAPSHOT,
                                "condition-node",
                            ))
                        } else if charge(
                            &mut snapshot_body_conditions,
                            body_conditions,
                            MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT,
                        ) {
                            Some((
                                RuleErrorCode::Condition,
                                MAX_RULE_BODY_CONDITIONS_PER_SNAPSHOT,
                                "body-condition",
                            ))
                        } else {
                            None
                        };
                        if let Some((code, limit, label)) = over_budget {
                            push_parse_diagnostic(
                                &mut errors,
                                parse_error(
                                    code,
                                    group,
                                    line_no,
                                    format!("snapshot exceeds the {limit}-{label} limit"),
                                ),
                            );
                            break 'groups;
                        }
                        rules.push(rule);
                    }
                    Err(error) => {
                        if push_parse_diagnostic(
                            &mut errors,
                            parse_error_with_span(
                                error.code,
                                group,
                                line_no,
                                error.span,
                                error.source.to_string(),
                            ),
                        ) {
                            break 'groups;
                        }
                    }
                }
            }
        }

        if errors.is_empty() {
            let index = RuleIndex::build(&rules);
            Ok(Self {
                language_version: RULE_LANGUAGE_VERSION,
                version: next_ruleset_version(),
                rules,
                index,
            })
        } else {
            Err(errors)
        }
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
        let mut single_families = ActionFamilySet::EMPTY;
        let mut skip = SkipState::default();
        let response = res.cloned().map(Arc::new);
        let condition_cache = matcher::ConditionCache::new(req);

        let mut ordered: Vec<usize> = self.candidate_rule_indices(parts.as_ref().ok(), &req.url);
        ordered.sort_by_key(|idx| {
            let rule = &self.rules[*idx];
            (!rule.important, *idx)
        });

        for idx in ordered {
            let rule = &self.rules[idx];
            let resources = &self.index.compiled_resources[idx];
            if rule.disabled {
                continue;
            }
            let captures = match parts.as_ref().ok().and_then(|url| {
                rule.matcher.matches_compiled(
                    url,
                    &req.url,
                    &self.index.compiled_globs,
                    &resources.matcher,
                )
            }) {
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

            let condition_context = matcher::ConditionMatchContext::compiled(
                parts.as_ref().ok(),
                res,
                rule.line,
                &self.index.compiled_globs,
                &self.index.compiled_body_literals,
                &condition_cache,
            );
            if !rule
                .conditions
                .iter()
                .zip(&resources.conditions)
                .all(|(condition, resources)| {
                    condition.matches_with_compiled(resources, &condition_context)
                })
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
                if skip.suppresses(family) {
                    continue;
                }
                if action.is_single() && single_families.contains(family) {
                    continue;
                }
                if action.is_single() {
                    single_families.insert(family);
                }
                if seen_rules.insert((brief.group.clone(), brief.line)) {
                    matched_rules.push(brief.clone());
                }
                skip.observe(&action);
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

/// Adds `cost` to a snapshot budget counter, reporting whether it now exceeds `limit`.
fn charge(counter: &mut usize, cost: usize, limit: usize) -> bool {
    *counter = counter.saturating_add(cost);
    *counter > limit
}

fn parse_error(code: RuleErrorCode, group: &str, line: usize, message: String) -> RuleError {
    parse_error_with_span(code, group, line, None, message)
}

fn parse_error_with_span(
    code: RuleErrorCode,
    group: &str,
    line: usize,
    span: Option<RuleSourceSpan>,
    message: String,
) -> RuleError {
    RuleError {
        code,
        group: group.to_string(),
        line,
        span,
        message,
    }
}

fn is_language_directive(line: &str) -> bool {
    line == "@language"
        || line
            .strip_prefix("@language")
            .is_some_and(|suffix| suffix.chars().next().is_some_and(char::is_whitespace))
}
