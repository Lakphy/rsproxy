use super::*;

pub(super) fn parse_condition(input: &str) -> Result<Condition, RuleModelError> {
    if let Some(rest) = input.strip_prefix('!') {
        return Ok(Condition::Not(Box::new(parse_condition(rest)?)));
    }
    let (name, args) = parse_call(input)?;
    match name {
        "method" => parse_method_condition(&args).map(Condition::Method),
        "host" => Ok(Condition::Host(unquote(require_one(&args, "host")?))),
        "url" => parse_url_condition(require_one(&args, "url")?).map(Condition::Url),
        "ip" | "clientIp" | "client_ip" | "client-ip" => {
            parse_ip_patterns(&args, "clientIp").map(Condition::ClientIp)
        }
        "serverIp" | "server_ip" | "server-ip" => {
            parse_ip_patterns(&args, "serverIp").map(Condition::ServerIp)
        }
        "header" => parse_header_condition(require_one(&args, "header")?, false),
        "res.header" | "resHeader" | "res_header" | "res-header" => {
            parse_header_condition(require_one(&args, "res.header")?, true)
        }
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
                .map(|arg| parse_condition(arg.trim()))
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
                .map(|arg| parse_condition(arg.trim()))
                .collect::<Result<Vec<_>, _>>()
                .map(Condition::All)
        }
        "not" => {
            let inner = require_one(&args, "not")?;
            Ok(Condition::Not(Box::new(parse_condition(inner)?)))
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
            if (100..=999).contains(&status) {
                Ok(status)
            } else {
                Err(RuleModelError::constraint(
                    "status condition",
                    format!("invalid status condition `{value}`"),
                ))
            }
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
    if name.is_empty() || name.chars().any(char::is_control) {
        Err(RuleModelError::invalid(
            "environment condition name",
            "env condition name is empty or contains control characters",
        ))
    } else {
        Ok(())
    }
}

fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn parse_url_condition(input: &str) -> Result<UrlCondition, RuleModelError> {
    let input = input.trim();
    if input.starts_with('/') && regex_literal_end(input).is_some() {
        parse_regex_matcher(input).map(UrlCondition::Regex)
    } else {
        Ok(UrlCondition::Glob(unquote(input)))
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
