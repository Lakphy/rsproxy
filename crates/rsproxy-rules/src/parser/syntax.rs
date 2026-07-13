use super::*;

pub(crate) fn parse_call(input: &str) -> Result<(&str, Vec<&str>), RuleModelError> {
    let open = input
        .find('(')
        .ok_or_else(|| RuleModelError::syntax("call", format!("expected call syntax: {input}")))?;
    if !input.ends_with(')') {
        return Err(RuleModelError::syntax(
            "call",
            format!("call must end with `)`: {input}"),
        ));
    }
    let name = &input[..open];
    if name.is_empty() {
        return Err(RuleModelError::empty("call name", "call name is empty"));
    }
    let args = split_args(&input[open + 1..input.len() - 1]);
    Ok((name, args))
}

pub(crate) fn require_one<'a>(
    args: &'a [&'a str],
    action: &str,
) -> Result<&'a str, RuleModelError> {
    if args.len() == 1 && !args[0].trim().is_empty() {
        Ok(args[0].trim())
    } else {
        Err(RuleModelError::missing(
            "action argument",
            format!("{action} requires exactly one argument"),
        ))
    }
}

pub(crate) fn require_call_body<'a>(
    input: &'a str,
    action: &str,
) -> Result<&'a str, RuleModelError> {
    let open = input
        .find('(')
        .ok_or_else(|| RuleModelError::syntax("call", format!("expected call syntax: {input}")))?;
    let body = input[open + 1..input.len() - 1].trim();
    if body.is_empty() {
        Err(RuleModelError::missing(
            "action argument",
            format!("{action} requires exactly one argument"),
        ))
    } else {
        Ok(body)
    }
}

pub(crate) fn parse_value(input: &str) -> Result<Value, RuleModelError> {
    let input = input.trim();
    if let Some(path) = input.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        if path.trim().is_empty() {
            Err(RuleModelError::empty(
                "file value path",
                "file value path must not be empty",
            ))
        } else {
            Ok(Value::File(path.to_string()))
        }
    } else if let Some(key) = input.strip_prefix('@') {
        if valid_value_key(key) {
            Ok(Value::Reference(key.to_string()))
        } else {
            Err(RuleModelError::invalid(
                "value key",
                format!(
                    "invalid value key `{key}`; use 1-128 ASCII letters, digits, dot, underscore, or hyphen"
                ),
            ))
        }
    } else {
        Ok(Value::Inline(unquote(input)))
    }
}

pub(crate) fn parse_duration_ms(input: &str) -> Result<u64, RuleModelError> {
    if let Some(ms) = input.strip_suffix("ms") {
        ms.parse::<u64>().map_err(|source| {
            RuleModelError::integer(
                "duration",
                input,
                format!("invalid duration `{input}`"),
                source,
            )
        })
    } else if let Some(sec) = input.strip_suffix('s') {
        let value = sec.parse::<f64>().map_err(|source| {
            RuleModelError::float(
                "duration",
                input,
                format!("invalid duration `{input}`"),
                source,
            )
        })?;
        Ok((value * 1000.0).round() as u64)
    } else {
        input.parse::<u64>().map_err(|source| {
            RuleModelError::integer(
                "duration",
                input,
                format!("invalid duration `{input}`"),
                source,
            )
        })
    }
}

pub(crate) fn parse_speed_bps(input: &str) -> Result<u64, RuleModelError> {
    let lower = input.to_ascii_lowercase();
    let raw = lower.strip_suffix("/s").unwrap_or(&lower);
    let (number, multiplier) = if let Some(value) = raw.strip_suffix("kb") {
        (value, 1024.0)
    } else if let Some(value) = raw.strip_suffix("k") {
        (value, 1024.0)
    } else if let Some(value) = raw.strip_suffix("mb") {
        (value, 1024.0 * 1024.0)
    } else if let Some(value) = raw.strip_suffix("m") {
        (value, 1024.0 * 1024.0)
    } else if let Some(value) = raw.strip_suffix('b') {
        (value, 1.0)
    } else {
        (raw, 1.0)
    };
    let value = number.parse::<f64>().map_err(|source| {
        RuleModelError::float("speed", input, format!("invalid speed `{input}`"), source)
    })?;
    let bytes = (value * multiplier).round() as u64;
    if bytes == 0 {
        return Err(RuleModelError::constraint(
            "speed",
            "speed must be greater than zero",
        ));
    }
    Ok(bytes)
}

pub(crate) fn tokenize(input: &str) -> Result<Vec<String>, RuleModelError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut paren_depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            current.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                current.push(ch);
            }
            ')' => {
                if paren_depth == 0 {
                    return Err(RuleModelError::syntax("rule", "unmatched `)`"));
                }
                paren_depth -= 1;
                current.push(ch);
            }
            c if c.is_whitespace() && paren_depth == 0 => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err(RuleModelError::syntax("rule", "unterminated quote"));
    }
    if paren_depth != 0 {
        return Err(RuleModelError::syntax("rule", "unclosed `(`"));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

pub(crate) fn split_args(input: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let mut angle_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut escaped = false;

    for (idx, ch) in input.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '<' => angle_depth += 1,
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            ',' if angle_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && paren_depth == 0 =>
            {
                args.push(input[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    let tail = input[start..].trim();
    if !tail.is_empty() {
        args.push(tail);
    }
    args
}

pub(crate) fn strip_comment(input: &str) -> Option<String> {
    let mut out = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            out.push(ch);
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            out.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                out.push(ch);
            }
            '#' => break,
            _ => out.push(ch),
        }
    }
    (!out.trim().is_empty()).then_some(out)
}

pub(crate) fn unquote(input: &str) -> String {
    let input = input.trim();
    let quoted = (input.starts_with('"') && input.ends_with('"'))
        || (input.starts_with('\'') && input.ends_with('\''));
    if !quoted || input.len() < 2 {
        return input.to_string();
    }
    let inner = &input[1..input.len() - 1];
    let mut out = String::new();
    let mut escaped = false;
    for ch in inner.chars() {
        if escaped {
            match ch {
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                other => out.push(other),
            }
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else {
            out.push(ch);
        }
    }
    out
}
