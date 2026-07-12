use super::*;

pub(super) fn parse_condition(input: &str) -> Result<Condition, String> {
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
            let value = raw
                .parse::<f64>()
                .map_err(|_| "chance must be 0.0..1.0".to_string())?;
            if !(0.0..=1.0).contains(&value) {
                return Err("chance must be 0.0..1.0".to_string());
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
                return Err("any requires at least one condition".to_string());
            }
            args.iter()
                .map(|arg| parse_condition(arg.trim()))
                .collect::<Result<Vec<_>, _>>()
                .map(Condition::Any)
        }
        _ => Err(format!("unknown condition `{name}`")),
    }
}

fn parse_method_condition(args: &[&str]) -> Result<Vec<String>, String> {
    if args.is_empty() {
        return Err("method requires at least one method".to_string());
    }
    args.iter()
        .map(|value| {
            let value = unquote(value);
            let value = value.trim();
            if value.is_empty() || !value.bytes().all(is_http_token_byte) {
                Err(format!("invalid method condition `{value}`"))
            } else {
                Ok(value.to_ascii_uppercase())
            }
        })
        .collect()
}

fn parse_ip_patterns(args: &[&str], name: &str) -> Result<Vec<String>, String> {
    if args.is_empty() {
        return Err(format!("{name} requires at least one pattern"));
    }
    args.iter()
        .map(|value| {
            let value = unquote(value);
            let value = value.trim();
            if value.is_empty() || value.chars().any(char::is_control) {
                Err(format!("{name} patterns must be non-empty"))
            } else {
                Ok(value.to_string())
            }
        })
        .collect()
}

fn parse_header_condition(input: &str, response: bool) -> Result<Condition, String> {
    if let Some((name, value)) = input.split_once('~') {
        let name = validate_header_name(name)?;
        let value = unquote(value.trim());
        if value.is_empty() {
            return Err("header contains condition value is empty".to_string());
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

fn parse_status_condition(args: &[&str]) -> Result<Vec<u16>, String> {
    if args.is_empty() {
        return Err("status requires at least one code".to_string());
    }
    args.iter()
        .map(|value| {
            let status = value
                .trim()
                .parse::<u16>()
                .map_err(|_| format!("invalid status condition `{value}`"))?;
            if (100..=999).contains(&status) {
                Ok(status)
            } else {
                Err(format!("invalid status condition `{value}`"))
            }
        })
        .collect()
}

fn validate_header_name(input: &str) -> Result<String, String> {
    let name = input.trim();
    if name.is_empty() || !name.bytes().all(is_http_token_byte) {
        Err(format!("invalid header condition name `{name}`"))
    } else {
        Ok(name.to_ascii_lowercase())
    }
}

fn validate_env_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().any(char::is_control) {
        Err("env condition name is empty or contains control characters".to_string())
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

fn parse_url_condition(input: &str) -> Result<UrlCondition, String> {
    let input = input.trim();
    if input.starts_with('/') && regex_literal_end(input).is_some() {
        parse_regex_matcher(input).map(UrlCondition::Regex)
    } else {
        Ok(UrlCondition::Glob(unquote(input)))
    }
}

fn parse_body_condition(input: &str) -> Result<Condition, String> {
    let input = input.trim();
    if input.starts_with('/') && regex_literal_end(input).is_some() {
        parse_regex_matcher(input).map(Condition::BodyRegex)
    } else {
        let text = input.strip_prefix('~').unwrap_or(input).trim();
        if text.is_empty() {
            return Err("body condition requires text or regex".to_string());
        }
        Ok(Condition::BodyContains(unquote(text)))
    }
}
