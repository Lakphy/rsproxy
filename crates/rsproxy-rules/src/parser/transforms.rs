use super::*;

pub(super) fn parse_query_ops(args: &[&str]) -> Result<Vec<QueryOp>, String> {
    if args.is_empty() {
        return Err("url.query requires at least one operation".to_string());
    }
    let mut ops = Vec::new();
    for arg in args {
        let arg = arg.trim();
        if let Some(name) = arg.strip_prefix('-') {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err("url.query remove op needs a name".to_string());
            }
            ops.push(QueryOp::Remove { name });
        } else {
            let (name, value) = arg
                .split_once('=')
                .ok_or_else(|| "url.query op must be `k=v` or `-k`".to_string())?;
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err("url.query name is empty".to_string());
            }
            ops.push(QueryOp::Set {
                name,
                value: parse_value(value.trim())?,
            });
        }
    }
    Ok(ops)
}

pub(super) fn parse_body_replace(args: &[&str], action: &str) -> Result<BodyOp, String> {
    if args.len() != 2 {
        return Err(format!("{action} requires pattern and replacement"));
    }
    Ok(BodyOp::Replace {
        pattern: parse_regex_replace_pattern(args[0])?,
        replacement: unquote(args[1]),
    })
}

pub(super) fn parse_inject_op(args: &[&str]) -> Result<InjectOp, String> {
    if !(2..=3).contains(&args.len()) {
        return Err("inject requires target, value, and optional mode".to_string());
    }
    let target = match args[0].trim().to_ascii_lowercase().as_str() {
        "html" => InjectTarget::Html,
        "js" | "javascript" => InjectTarget::Js,
        "css" => InjectTarget::Css,
        other => return Err(format!("unsupported inject target `{other}`")),
    };
    let mode = match args.get(2).map(|value| value.trim().to_ascii_lowercase()) {
        None => InjectMode::Append,
        Some(mode) if mode == "append" => InjectMode::Append,
        Some(mode) if mode == "prepend" => InjectMode::Prepend,
        Some(mode) if mode == "replace" => InjectMode::Replace,
        Some(other) => return Err(format!("unsupported inject mode `{other}`")),
    };
    Ok(InjectOp {
        target,
        value: parse_value(args[1])?,
        mode,
    })
}

pub(super) fn parse_url_rewrite_pattern(input: &str) -> Result<UrlRewritePattern, String> {
    let input = input.trim();
    if input.starts_with('/') && regex_literal_end(input).is_some() {
        return parse_regex_replace_pattern(input).map(UrlRewritePattern::Regex);
    }
    Ok(UrlRewritePattern::Plain(parse_value(input)?))
}

fn parse_regex_replace_pattern(input: &str) -> Result<RegexReplacePattern, String> {
    let input = input.trim();
    if !input.starts_with('/') {
        return RegexReplacePattern::new(unquote(input), false)
            .map_err(|error| format!("invalid replacement regex: {error}"));
    }

    let end = regex_literal_end(input)
        .ok_or_else(|| "body.replace regex pattern must end with `/`".to_string())?;
    let flags = &input[end + 1..];
    if flags.chars().any(|ch| ch != 'i') {
        return Err(format!("unsupported body.replace regex flags `{flags}`"));
    }
    RegexReplacePattern::new(input[1..end].to_string(), flags.contains('i'))
        .map_err(|error| format!("invalid replacement regex: {error}"))
}

pub(super) fn regex_literal_end(input: &str) -> Option<usize> {
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
    end
}
