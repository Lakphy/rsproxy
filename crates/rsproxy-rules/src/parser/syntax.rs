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
    let args = split_args(&input[open + 1..input.len() - 1])?;
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
    if input.starts_with('<') != input.ends_with('>') {
        return Err(RuleModelError::syntax(
            "file value",
            "file values must use a paired `<path>` delimiter",
        ));
    }
    if let Some(path) = input.strip_prefix('<').and_then(|s| s.strip_suffix('>')) {
        if path.trim().is_empty() {
            Err(RuleModelError::empty(
                "file value path",
                "file value path must not be empty",
            ))
        } else if path.contains('\0') {
            Err(RuleModelError::invalid(
                "file value path",
                "file value path must not contain NUL",
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
        checked_scaled_number(value, 1000.0, "duration", input, true)
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
    let bytes = checked_scaled_number(value, multiplier, "speed", input, false)?;
    if bytes == 0 {
        return Err(RuleModelError::constraint(
            "speed",
            "speed must be greater than zero",
        ));
    }
    Ok(bytes)
}

fn checked_scaled_number(
    value: f64,
    multiplier: f64,
    context: &'static str,
    input: &str,
    allow_zero: bool,
) -> Result<u64, RuleModelError> {
    let scaled = value * multiplier;
    if !value.is_finite()
        || value.is_sign_negative()
        || !scaled.is_finite()
        || scaled >= u64::MAX as f64
        || (!allow_zero && scaled < 0.5)
    {
        return Err(RuleModelError::constraint(
            context,
            format!("{context} `{input}` is outside the supported finite range"),
        ));
    }
    Ok(scaled.round() as u64)
}

pub(crate) fn split_args(input: &str) -> Result<Vec<&str>, RuleModelError> {
    if input.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut args = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<char> = None;
    let mut angle_depth = 0usize;
    let mut brace_depth = 0usize;
    let mut bracket_depth = 0usize;
    let mut paren_depth = 0usize;
    let mut escaped = false;
    let mut regex_end = None;

    // Each depth is bounded by the input length, so the sum cannot overflow.
    let check_nesting = |angle: usize, brace: usize, bracket: usize, paren: usize| {
        if angle + brace + bracket + paren > MAX_PARSE_NESTING {
            Err(RuleModelError::limit(
                "call nesting",
                format!("call nesting exceeds {MAX_PARSE_NESTING} levels"),
            ))
        } else {
            Ok(())
        }
    };

    for (idx, ch) in input.char_indices() {
        if let Some(end) = regex_end {
            if idx == end {
                regex_end = None;
            }
            continue;
        }
        if ch == '/'
            && begins_regex_value(input, idx)
            && let Some(end) = regex_end_at_argument_boundary(input, idx)
        {
            regex_end = Some(end);
            continue;
        }
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
            '(' => {
                paren_depth += 1;
                check_nesting(angle_depth, brace_depth, bracket_depth, paren_depth)?;
            }
            ')' => {
                if paren_depth == 0 {
                    return Err(RuleModelError::syntax("call argument", "unmatched `)`"));
                }
                paren_depth -= 1;
            }
            '<' if begins_file_value(input, idx) => {
                angle_depth += 1;
                check_nesting(angle_depth, brace_depth, bracket_depth, paren_depth)?;
            }
            '>' if angle_depth > 0 => angle_depth -= 1,
            '{' => {
                brace_depth += 1;
                check_nesting(angle_depth, brace_depth, bracket_depth, paren_depth)?;
            }
            '}' => {
                if brace_depth == 0 {
                    return Err(RuleModelError::syntax("call argument", "unmatched `}`"));
                }
                brace_depth -= 1;
            }
            '[' => {
                bracket_depth += 1;
                check_nesting(angle_depth, brace_depth, bracket_depth, paren_depth)?;
            }
            ']' => {
                if bracket_depth == 0 {
                    return Err(RuleModelError::syntax("call argument", "unmatched `]`"));
                }
                bracket_depth -= 1;
            }
            ',' if angle_depth == 0
                && brace_depth == 0
                && bracket_depth == 0
                && paren_depth == 0 =>
            {
                push_argument(
                    &mut args,
                    input[start..idx].trim(),
                    "call arguments must not be empty",
                )?;
                start = idx + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return Err(RuleModelError::syntax(
            "call argument",
            "unterminated quote",
        ));
    }
    for (depth, delimiter) in [
        (angle_depth, '<'),
        (brace_depth, '{'),
        (bracket_depth, '['),
        (paren_depth, '('),
    ] {
        if depth != 0 {
            return Err(RuleModelError::syntax(
                "call argument",
                format!("unclosed `{delimiter}`"),
            ));
        }
    }
    push_argument(
        &mut args,
        input[start..].trim(),
        "call arguments must not be empty or end with a comma",
    )?;
    Ok(args)
}

fn push_argument<'a>(
    args: &mut Vec<&'a str>,
    argument: &'a str,
    empty_message: &'static str,
) -> Result<(), RuleModelError> {
    if argument.is_empty() {
        return Err(RuleModelError::empty("call argument", empty_message));
    }
    if args.len() == MAX_RULE_CALL_ARGUMENTS {
        return Err(RuleModelError::limit(
            "call argument count",
            format!("call exceeds the {MAX_RULE_CALL_ARGUMENTS}-argument limit"),
        ));
    }
    args.push(argument);
    Ok(())
}

/// Returns the last non-whitespace character before `index`, if any.
fn last_non_whitespace_before(input: &str, index: usize) -> Option<char> {
    input[..index]
        .chars()
        .rev()
        .find(|character| !character.is_whitespace())
}

fn begins_file_value(input: &str, index: usize) -> bool {
    last_non_whitespace_before(input, index)
        .is_none_or(|character| matches!(character, ',' | '(' | '='))
}

fn begins_regex_value(input: &str, index: usize) -> bool {
    last_non_whitespace_before(input, index)
        .is_none_or(|character| matches!(character, ',' | '(' | '=' | '~'))
}

/// Finds a closing regex delimiter only when it is followed by an argument
/// boundary. This keeps `/a,b/` as one regex value without mistaking the two
/// ordinary path arguments in `url.rewrite(/old, /new)` for one regex.
fn regex_end_at_argument_boundary(input: &str, start: usize) -> Option<usize> {
    let mut escaped = false;
    let mut character_class = false;
    for (offset, character) in input[start + 1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '[' {
            character_class = true;
            continue;
        }
        if character == ']' && character_class {
            character_class = false;
            continue;
        }
        if character == '/' && !character_class {
            let end = start + 1 + offset;
            if input[end + 1..]
                .chars()
                .find(|character| !character.is_whitespace())
                .is_none_or(|character| character == ',')
            {
                return Some(end);
            }
        }
    }
    None
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
