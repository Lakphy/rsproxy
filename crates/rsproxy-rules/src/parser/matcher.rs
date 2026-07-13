use super::*;

pub(super) fn parse_matcher(input: &str) -> Result<Matcher, RuleModelError> {
    if let Some(rest) = input.strip_prefix('!') {
        return Ok(Matcher::Not(Box::new(parse_matcher(rest)?)));
    }
    if let Some(rest) = input.strip_prefix('=') {
        if rest.is_empty() {
            return Err(RuleModelError::missing(
                "exact matcher",
                "exact matcher must include a URL",
            ));
        }
        UrlParts::parse(rest).map_err(|source| RuleModelError::InvalidExactUrlMatcher {
            source: Box::new(source),
        })?;
        parse_glob_matcher(rest).map_err(|source| RuleModelError::InvalidExactUrlMatcher {
            source: Box::new(source),
        })?;
        return Ok(Matcher::ExactUrl(rest.to_string()));
    }
    if input.starts_with('/') {
        return parse_regex_matcher(input).map(Matcher::Regex);
    }
    if let Some(port) = input.strip_prefix(':') {
        let port = port.parse::<u16>().map_err(|source| {
            RuleModelError::integer(
                "port matcher",
                port,
                format!("invalid port matcher: {input}"),
                source,
            )
        })?;
        if port == 0 {
            return Err(RuleModelError::constraint(
                "port matcher",
                "port must be 1..65535",
            ));
        }
        return Ok(Matcher::Port(port));
    }
    Ok(Matcher::Glob(parse_glob_matcher(input)?))
}

fn parse_glob_matcher(input: &str) -> Result<GlobMatcher, RuleModelError> {
    let (scheme, rest) = match input.split_once("://") {
        Some((scheme, rest)) if valid_scheme(scheme) => (Some(scheme.to_ascii_lowercase()), rest),
        Some((scheme, _)) => {
            return Err(RuleModelError::invalid(
                "matcher scheme",
                format!("invalid matcher scheme `{scheme}`"),
            ));
        }
        None => (None, input),
    };
    let (before_query, query) = match rest.split_once('?') {
        Some((left, right)) => (left, Some(right.to_string())),
        None => (rest, None),
    };
    let (host_port, path) = match before_query.find('/') {
        Some(idx) => (&before_query[..idx], Some(before_query[idx..].to_string())),
        None => (before_query, None),
    };
    if host_port.is_empty() {
        return Err(RuleModelError::empty(
            "glob matcher host",
            "glob matcher host is empty",
        ));
    }
    let (host, port) = parse_glob_authority(host_port)?;
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if host.is_empty() {
        return Err(RuleModelError::empty(
            "glob matcher host",
            "glob matcher host is empty",
        ));
    }

    Ok(GlobMatcher {
        scheme,
        host,
        port,
        path,
        query,
    })
}

fn parse_glob_authority(input: &str) -> Result<(&str, Option<String>), RuleModelError> {
    if input.starts_with('[') {
        let end = input.find(']').ok_or_else(|| {
            RuleModelError::syntax("IPv6 matcher", "IPv6 matcher is missing closing `]`")
        })?;
        let host = &input[..=end];
        let tail = &input[end + 1..];
        let port = if tail.is_empty() {
            None
        } else {
            let raw = tail.strip_prefix(':').ok_or_else(|| {
                RuleModelError::invalid(
                    "matcher authority",
                    format!("invalid matcher authority `{input}`"),
                )
            })?;
            Some(parse_port_pattern(raw)?)
        };
        return Ok((host, port));
    }
    if input.contains(['[', ']']) {
        return Err(RuleModelError::invalid(
            "matcher authority",
            format!("invalid matcher authority `{input}`"),
        ));
    }
    let Some((host, raw_port)) = input.rsplit_once(':') else {
        return Ok((input, None));
    };
    if host.contains(':') {
        return Err(RuleModelError::syntax(
            "IPv6 matcher",
            "IPv6 matcher must use brackets",
        ));
    }
    Ok((host, Some(parse_port_pattern(raw_port)?)))
}

fn parse_port_pattern(input: &str) -> Result<String, RuleModelError> {
    if input.is_empty() {
        return Err(RuleModelError::empty(
            "matcher port",
            "matcher port is empty",
        ));
    }
    if input.contains('*') {
        if input
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'*')
        {
            return Ok(input.to_string());
        }
        return Err(RuleModelError::invalid(
            "matcher port pattern",
            format!("invalid matcher port pattern `{input}`"),
        ));
    }
    let port = input.parse::<u16>().map_err(|source| {
        RuleModelError::integer(
            "matcher port",
            input,
            format!("invalid matcher port `{input}`"),
            source,
        )
    })?;
    if port == 0 {
        Err(RuleModelError::constraint(
            "matcher port",
            "matcher port must be 1..65535",
        ))
    } else {
        Ok(port.to_string())
    }
}

fn valid_scheme(input: &str) -> bool {
    let mut bytes = input.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'))
}

pub(super) fn parse_regex_matcher(input: &str) -> Result<RegexMatcher, RuleModelError> {
    let mut escaped = false;
    let mut end = None;
    for (idx, ch) in input.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '/' {
            end = Some(idx);
        }
    }
    let end = end.ok_or_else(|| {
        RuleModelError::syntax("regex matcher", "regex matcher must end with `/`")
    })?;
    let pattern = &input[1..end];
    let flags = &input[end + 1..];
    if flags.chars().any(|ch| ch != 'i') {
        return Err(RuleModelError::unsupported(
            "regex flags",
            format!("unsupported regex flags `{flags}`"),
        ));
    }
    let case_insensitive = flags.contains('i');
    let (engine, compiled) = match LinearRegexBuilder::new(pattern)
        .case_insensitive(case_insensitive)
        .build()
    {
        Ok(regex) => (RegexEngine::Linear, Arc::new(CompiledRegex::Linear(regex))),
        Err(linear_err) => {
            let regex = FancyRegexBuilder::new(&fancy_pattern(pattern, case_insensitive))
                .backtrack_limit(DEFAULT_FANCY_BACKTRACK_LIMIT)
                .build()
                .map_err(|fancy| RuleModelError::InvalidRegexMatcher {
                    linear: Box::new(linear_err),
                    fancy: Box::new(fancy),
                })?;
            (RegexEngine::Fancy, Arc::new(CompiledRegex::Fancy(regex)))
        }
    };
    Ok(RegexMatcher {
        pattern: pattern.to_string(),
        case_insensitive,
        engine,
        compiled,
    })
}

fn fancy_pattern(pattern: &str, case_insensitive: bool) -> String {
    if case_insensitive {
        format!("(?i:{pattern})")
    } else {
        pattern.to_string()
    }
}
