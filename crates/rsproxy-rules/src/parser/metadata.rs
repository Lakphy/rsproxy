use super::*;

pub(super) fn parse_header_op(input: &str) -> Result<HeaderOp, String> {
    let input = input.trim();
    if let Some(name) = input.strip_prefix('-') {
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err("header remove op needs a name".to_string());
        }
        return Ok(HeaderOp::Remove { name });
    }
    if let Some((name, expression)) = header_replace_parts(input) {
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            return Err("header name is empty".to_string());
        }
        let (pattern, replacement) = parse_header_replacement(expression)?;
        return Ok(HeaderOp::Replace {
            name,
            pattern,
            replacement,
        });
    }
    let (name, value) = input.split_once(':').ok_or_else(|| {
        "header op must be `name: value`, `-name`, or `name ~ /regex/replacement`".to_string()
    })?;
    let name = name.trim().to_ascii_lowercase();
    if name.is_empty() {
        return Err("header name is empty".to_string());
    }
    Ok(HeaderOp::Set {
        name,
        value: parse_value(value.trim())?,
    })
}

fn header_replace_parts(input: &str) -> Option<(&str, &str)> {
    let tilde = input.find('~')?;
    if input.find(':').is_some_and(|colon| colon < tilde) {
        return None;
    }
    Some((&input[..tilde], &input[tilde + 1..]))
}

fn parse_header_replacement(expression: &str) -> Result<(RegexReplacePattern, String), String> {
    let expression = expression.trim();
    if !expression.starts_with('/') {
        return Err("header replacement must use `/regex/replacement`".to_string());
    }
    let separator = first_unescaped_slash(expression, 1)
        .ok_or_else(|| "header replacement must use `/regex/replacement`".to_string())?;
    let pattern = unescape_slashes(&expression[1..separator]);
    let replacement = unescape_slashes(&expression[separator + 1..]);
    let pattern = RegexReplacePattern::new(pattern, false)
        .map_err(|error| format!("invalid header replacement regex: {error}"))?;
    Ok((pattern, replacement))
}

fn first_unescaped_slash(input: &str, start: usize) -> Option<usize> {
    let mut escaped = false;
    for (offset, character) in input[start..].char_indices() {
        if escaped {
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == '/' {
            return Some(start + offset);
        }
    }
    None
}

fn unescape_slashes(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut characters = input.chars().peekable();
    while let Some(character) = characters.next() {
        if character == '\\' && characters.peek() == Some(&'/') {
            characters.next();
            output.push('/');
        } else {
            output.push(character);
        }
    }
    output
}

pub(super) fn parse_cookie_op(input: &str) -> Result<CookieOp, String> {
    let input = input.trim();
    if let Some(name) = input.strip_prefix('-') {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err("cookie remove op needs a name".to_string());
        }
        return Ok(CookieOp::Remove { name });
    }
    let mut parts = input.split(';');
    let first = parts
        .next()
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .ok_or_else(|| "cookie op must be `name=value` or `-name`".to_string())?;
    let (name, value) = first
        .split_once('=')
        .ok_or_else(|| "cookie op must be `name=value` or `-name`".to_string())?;
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("cookie name is empty".to_string());
    }
    let mut attrs = Vec::new();
    for part in parts {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (attr_name, attr_value) = match part.split_once('=') {
            Some((name, value)) => (name.trim(), Some(parse_value(value.trim())?)),
            None => (part, None),
        };
        let attr_name = canonical_cookie_attr_name(attr_name.trim());
        if attr_name.is_empty() {
            return Err("cookie attribute name is empty".to_string());
        }
        attrs.push(CookieAttr {
            name: attr_name,
            value: attr_value,
        });
    }
    Ok(CookieOp::Set {
        name,
        value: parse_value(value.trim())?,
        attrs,
    })
}

fn canonical_cookie_attr_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "path" => "Path".to_string(),
        "domain" => "Domain".to_string(),
        "expires" => "Expires".to_string(),
        "max-age" | "max_age" => "Max-Age".to_string(),
        "http-only" | "httponly" => "HttpOnly".to_string(),
        "secure" => "Secure".to_string(),
        "samesite" | "same-site" | "same_site" => "SameSite".to_string(),
        "partitioned" => "Partitioned".to_string(),
        "priority" => "Priority".to_string(),
        other => other
            .split('-')
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join("-"),
    }
}

pub(super) fn parse_cors_op(args: &[&str]) -> Result<CorsOp, String> {
    if args.is_empty() {
        return Err("res.cors requires at least an origin".to_string());
    }
    let mut origin = None;
    let mut methods = None;
    let mut headers = None;
    let mut credentials = None;
    let mut expose = None;
    let mut max_age = None;

    for (idx, arg) in args.iter().enumerate() {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        if idx == 0 && !arg.contains('=') {
            origin = Some(parse_value(arg)?);
            continue;
        }
        let (key, value) = arg
            .split_once('=')
            .ok_or_else(|| "res.cors detailed arguments must be key=value".to_string())?;
        let key = key.trim().to_ascii_lowercase();
        match key.as_str() {
            "origin" | "allow-origin" => origin = Some(parse_value(value.trim())?),
            "methods" | "allow-methods" => methods = Some(parse_value(value.trim())?),
            "headers" | "allow-headers" => headers = Some(parse_value(value.trim())?),
            "credentials" | "allow-credentials" => {
                credentials = Some(parse_bool(&unquote(value.trim()), "res.cors credentials")?)
            }
            "expose" | "expose-headers" => expose = Some(parse_value(value.trim())?),
            "max-age" | "max_age" => max_age = Some(parse_value(value.trim())?),
            _ => return Err(format!("unknown res.cors argument `{key}`")),
        }
    }

    let origin = origin.ok_or_else(|| "res.cors requires an origin".to_string())?;
    Ok(CorsOp {
        origin,
        methods,
        headers,
        credentials,
        expose,
        max_age,
    })
}

fn parse_bool(input: &str, field: &str) -> Result<bool, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => Ok(true),
        "false" | "no" | "0" | "off" => Ok(false),
        _ => Err(format!("{field} must be true or false")),
    }
}

pub(super) fn parse_cache_op(args: &[&str]) -> Result<CacheOp, String> {
    if args.is_empty() {
        return Err("cache requires at least one directive".to_string());
    }
    if args.len() == 1 && args[0].trim().eq_ignore_ascii_case("off") {
        return Ok(CacheOp::Off);
    }

    let mut directives = Vec::new();
    for arg in args {
        let arg = arg.trim();
        if arg.is_empty() {
            continue;
        }
        if arg.chars().all(|ch| ch.is_ascii_digit()) {
            directives.push(CacheDirective {
                name: "max-age".to_string(),
                value: Some(Value::inline(arg)),
            });
            continue;
        }
        let (name, value) = match arg.split_once('=') {
            Some((name, value)) => (name.trim(), Some(parse_value(value.trim())?)),
            None => (arg, None),
        };
        let name = canonical_cache_directive_name(name);
        if name.is_empty() {
            return Err("cache directive name is empty".to_string());
        }
        directives.push(CacheDirective { name, value });
    }

    if directives.is_empty() {
        return Err("cache requires at least one directive".to_string());
    }
    Ok(CacheOp::Directives(directives))
}

fn canonical_cache_directive_name(name: &str) -> String {
    match name.trim().to_ascii_lowercase().as_str() {
        "max_age" => "max-age".to_string(),
        "s_maxage" | "s-max-age" => "s-maxage".to_string(),
        "stale_while_revalidate" | "stale-while-revalidate" => "stale-while-revalidate".to_string(),
        "stale_if_error" | "stale-if-error" => "stale-if-error".to_string(),
        "must_revalidate" | "must-revalidate" => "must-revalidate".to_string(),
        "proxy_revalidate" | "proxy-revalidate" => "proxy-revalidate".to_string(),
        "no_cache" | "no-cache" => "no-cache".to_string(),
        "no_store" | "no-store" => "no-store".to_string(),
        "no_transform" | "no-transform" => "no-transform".to_string(),
        other => other.to_string(),
    }
}
