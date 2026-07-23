use super::*;

pub(crate) fn condition_node_count(condition: &Condition) -> usize {
    let mut count = 0usize;
    condition.for_each_node(&mut |_| count += 1);
    count
}

pub(crate) fn body_condition_count(condition: &Condition) -> usize {
    let mut count = 0usize;
    condition.for_each_node(&mut |node| {
        if matches!(node, Condition::BodyContains(_) | Condition::BodyRegex(_)) {
            count += 1;
        }
    });
    count
}

pub(super) fn parse_condition(
    input: &str,
    profile: SyntaxProfile,
) -> Result<Condition, RuleModelError> {
    parse_condition_at_depth(input, 0, profile)
}

fn parse_condition_at_depth(
    input: &str,
    depth: usize,
    profile: SyntaxProfile,
) -> Result<Condition, RuleModelError> {
    if depth > MAX_PARSE_NESTING {
        return Err(RuleModelError::limit(
            "condition nesting",
            format!("condition nesting exceeds {MAX_PARSE_NESTING} levels"),
        ));
    }
    if let Some(rest) = input.strip_prefix('!') {
        return Ok(Condition::Not(Box::new(parse_condition_at_depth(
            rest,
            depth + 1,
            profile,
        )?)));
    }
    let (input_name, args) = parse_call(input)?;
    let name = match profile {
        SyntaxProfile::Compatible => canonical_condition_name(input_name),
        SyntaxProfile::CanonicalV3 => language::canonical_v3_condition_name(input_name),
    };
    let Some(name) = name else {
        if profile == SyntaxProfile::CanonicalV3
            && let Some(canonical) = canonical_condition_name(input_name)
        {
            return Err(RuleModelError::unsupported(
                "condition",
                format!(
                    "v3 accepts canonical condition names only; replace `{input_name}` with `{canonical}`"
                ),
            ));
        }
        return Err(RuleModelError::unsupported(
            "condition",
            format!("unknown condition `{input_name}`"),
        ));
    };
    match name {
        "method" => parse_method_condition(&args).map(Condition::Method),
        "host" => {
            let pattern = unquote(require_one(&args, "host")?);
            validate_glob_pattern(&pattern, '.', "host condition glob")?;
            Ok(Condition::Host(pattern))
        }
        "url" => parse_url_condition(require_one(&args, "url")?).map(Condition::Url),
        "client.ip" => parse_ip_patterns(&args, "client.ip").map(Condition::ClientIp),
        "server.ip" => parse_ip_patterns(&args, "server.ip").map(Condition::ServerIp),
        "header" => parse_header_condition(require_one(&args, "header")?, false),
        "res.header" => parse_header_condition(require_one(&args, "res.header")?, true),
        "body" => parse_body_condition(require_one(&args, "body")?),
        "status" => parse_status_condition(&args).map(Condition::Status),
        "chance" => {
            let raw = require_one(&args, "chance")?;
            let value = raw.parse::<f64>().map_err(|source| {
                RuleModelError::float("chance condition", raw, "chance must be 0.0..1.0", source)
            })?;
            if !(0.0..=1.0).contains(&value) {
                return Err(RuleModelError::constraint(
                    "chance condition",
                    "chance must be 0.0..1.0",
                ));
            }
            Ok(Condition::ChancePermille((value * 1000.0).round() as u16))
        }
        "env" => {
            let arg = require_one(&args, "env")?;
            if let Some((name, value)) = arg.split_once('=') {
                let name = name.trim();
                validate_env_name(name)?;
                Ok(Condition::EnvEquals {
                    name: name.to_string(),
                    value: unquote(value.trim()),
                })
            } else {
                let name = arg.trim();
                validate_env_name(name)?;
                Ok(Condition::EnvPresent(name.to_string()))
            }
        }
        "any" => {
            if args.is_empty() {
                return Err(RuleModelError::missing(
                    "any condition",
                    "any requires at least one condition",
                ));
            }
            args.iter()
                .map(|arg| parse_condition_at_depth(arg.trim(), depth + 1, profile))
                .collect::<Result<Vec<_>, _>>()
                .map(Condition::Any)
        }
        "all" => {
            if args.is_empty() {
                return Err(RuleModelError::missing(
                    "all condition",
                    "all requires at least one condition",
                ));
            }
            args.iter()
                .map(|arg| parse_condition_at_depth(arg.trim(), depth + 1, profile))
                .collect::<Result<Vec<_>, _>>()
                .map(Condition::All)
        }
        "not" => {
            let inner = require_one(&args, "not")?;
            Ok(Condition::Not(Box::new(parse_condition_at_depth(
                inner,
                depth + 1,
                profile,
            )?)))
        }
        _ => Err(RuleModelError::unsupported(
            "condition",
            format!("unknown condition `{name}`"),
        )),
    }
}

fn parse_method_condition(args: &[&str]) -> Result<Vec<String>, RuleModelError> {
    if args.is_empty() {
        return Err(RuleModelError::missing(
            "method condition",
            "method requires at least one method",
        ));
    }
    args.iter()
        .map(|value| {
            let value = unquote(value);
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(is_http_token_byte) {
                Err(RuleModelError::invalid(
                    "method condition",
                    format!("invalid method condition `{value}`"),
                ))
            } else {
                Ok(value.to_ascii_uppercase())
            }
        })
        .collect()
}

fn parse_ip_patterns(args: &[&str], name: &str) -> Result<Vec<String>, RuleModelError> {
    if args.is_empty() {
        return Err(RuleModelError::missing(
            "IP condition",
            format!("{name} requires at least one pattern"),
        ));
    }
    args.iter()
        .map(|value| {
            let value = unquote(value);
            let value = value.trim();
            if value.is_empty() || value.chars().any(char::is_control) {
                Err(RuleModelError::empty(
                    "IP condition pattern",
                    format!("{name} patterns must be non-empty"),
                ))
            } else {
                validate_glob_pattern(value, '\0', "IP condition glob")?;
                Ok(value.to_string())
            }
        })
        .collect()
}

fn parse_header_condition(input: &str, response: bool) -> Result<Condition, RuleModelError> {
    if let Some((name, value)) = input.split_once('~') {
        let name = validate_header_name(name)?;
        let value = unquote(value.trim());
        if value.is_empty() {
            return Err(RuleModelError::empty(
                "header contains condition",
                "header contains condition value is empty",
            ));
        }
        if response {
            Ok(Condition::ResHeaderContains { name, value })
        } else {
            Ok(Condition::HeaderContains { name, value })
        }
    } else {
        let name = validate_header_name(input)?;
        if response {
            Ok(Condition::ResHeaderPresent(name))
        } else {
            Ok(Condition::HeaderPresent(name))
        }
    }
}

fn parse_status_condition(args: &[&str]) -> Result<Vec<u16>, RuleModelError> {
    if args.is_empty() {
        return Err(RuleModelError::missing(
            "status condition",
            "status requires at least one code",
        ));
    }
    args.iter()
        .map(|value| {
            let status = value.trim().parse::<u16>().map_err(|source| {
                RuleModelError::integer(
                    "status condition",
                    value.trim(),
                    format!("invalid status condition `{value}`"),
                    source,
                )
            })?;
            validate_status_range(
                status,
                MIN_HTTP_STATUS..=MAX_HTTP_STATUS,
                "status condition",
                "100..599",
            )?;
            Ok(status)
        })
        .collect()
}

fn validate_header_name(input: &str) -> Result<String, RuleModelError> {
    let name = input.trim();
    if name.is_empty() || !name.bytes().all(is_http_token_byte) {
        Err(RuleModelError::invalid(
            "header condition name",
            format!("invalid header condition name `{name}`"),
        ))
    } else {
        Ok(name.to_ascii_lowercase())
    }
}

fn validate_env_name(name: &str) -> Result<(), RuleModelError> {
    if name.is_empty()
        || name.contains('=')
        || name.contains('\0')
        || name.chars().any(char::is_whitespace)
    {
        Err(RuleModelError::invalid(
            "environment condition name",
            "env condition name is empty or contains `=`, NUL, or whitespace",
        ))
    } else {
        Ok(())
    }
}

fn parse_url_condition(input: &str) -> Result<UrlCondition, RuleModelError> {
    let input = input.trim();
    if input.starts_with('/') && regex_literal_end(input).is_some() {
        parse_regex_matcher(input).map(UrlCondition::Regex)
    } else {
        let pattern = unquote(input);
        validate_glob_pattern(&pattern, '\0', "URL condition glob")?;
        Ok(UrlCondition::Glob(pattern))
    }
}

fn parse_body_condition(input: &str) -> Result<Condition, RuleModelError> {
    let input = input.trim();
    if input.starts_with('/') && regex_literal_end(input).is_some() {
        parse_regex_matcher(input).map(Condition::BodyRegex)
    } else {
        let text = input.strip_prefix('~').unwrap_or(input).trim();
        if text.is_empty() {
            return Err(RuleModelError::missing(
                "body condition",
                "body condition requires text or regex",
            ));
        }
        Ok(Condition::BodyContains(unquote(text)))
    }
}
