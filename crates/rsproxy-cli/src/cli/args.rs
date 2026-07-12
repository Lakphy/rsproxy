use super::*;

pub(crate) fn option_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|idx| args.get(idx + 1).cloned())
}

pub(super) fn option_values(args: &[String], names: &[&str]) -> Vec<String> {
    let mut values = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if names.iter().any(|name| arg == name)
            && let Some(value) = iter.next()
        {
            values.push(value.clone());
        }
    }
    values
}

pub(crate) fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

pub(super) fn positional_skipping_values(
    args: &[String],
    value_options: &[&str],
) -> Option<String> {
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if value_options.iter().any(|name| arg == name) {
            skip_next = true;
            continue;
        }
        if !arg.starts_with('-') {
            return Some(arg.clone());
        }
    }
    None
}

pub(super) fn read_stdin() -> Result<String, String> {
    let mut input = String::new();
    io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| e.to_string())?;
    Ok(input)
}

pub(super) fn format_rule_errors(errors: Vec<rsproxy_rules::RuleError>) -> String {
    errors
        .into_iter()
        .map(|e| e.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

pub(super) fn parse_size(input: &str) -> Result<usize, String> {
    let lower = input.trim().to_ascii_lowercase();
    if let Some(raw) = lower.strip_suffix("kb") {
        Ok(raw
            .parse::<usize>()
            .map_err(|_| format!("invalid size `{input}`"))?
            * 1024)
    } else if let Some(raw) = lower.strip_suffix("mb") {
        Ok(raw
            .parse::<usize>()
            .map_err(|_| format!("invalid size `{input}`"))?
            * 1024
            * 1024)
    } else if let Some(raw) = lower.strip_suffix("gb") {
        Ok(raw
            .parse::<usize>()
            .map_err(|_| format!("invalid size `{input}`"))?
            * 1024
            * 1024
            * 1024)
    } else if let Some(raw) = lower.strip_suffix('b') {
        raw.parse::<usize>()
            .map_err(|_| format!("invalid size `{input}`"))
    } else {
        lower
            .parse::<usize>()
            .map_err(|_| format!("invalid size `{input}`"))
    }
}

pub(super) fn parse_positive_usize(input: &str, name: &str) -> Result<usize, String> {
    let value = input
        .trim()
        .parse::<usize>()
        .map_err(|_| format!("invalid {name}"))?;
    if value == 0 {
        return Err(format!("{name} must be greater than zero"));
    }
    Ok(value)
}

pub(super) fn parse_trace_spill_compression(
    input: &str,
) -> Result<rsproxy_trace::TraceSpillCompression, String> {
    let value = input.trim().to_ascii_lowercase();
    match value.as_str() {
        "none" | "off" | "false" => Ok(rsproxy_trace::TraceSpillCompression::None),
        "zstd" | "zstd:1" => Ok(rsproxy_trace::TraceSpillCompression::Zstd { level: 1 }),
        _ => {
            if let Some(level) = value.strip_prefix("zstd:") {
                let level = level
                    .parse::<i32>()
                    .map_err(|_| "invalid --trace-spill-compression zstd level".to_string())?;
                if !(1..=22).contains(&level) {
                    return Err(
                        "--trace-spill-compression zstd level must be between 1 and 22".to_string(),
                    );
                }
                Ok(rsproxy_trace::TraceSpillCompression::Zstd { level })
            } else {
                Err("--trace-spill-compression must be none or zstd[:level]".to_string())
            }
        }
    }
}

pub(super) fn apply_trace_filter(config: &mut AppConfig, input: &str) -> Result<(), String> {
    for raw in input.split(',') {
        let value = raw.trim().to_ascii_lowercase();
        if value.is_empty() {
            continue;
        }
        match value.as_str() {
            "headers-only" | "headers_only" | "headers" | "no-body" | "no_body" => {
                config.trace_body_limit = 0;
            }
            "media" | "media-body-off" | "media_body_off" | "no-media-body" | "no_media_body"
            | "exclude-media" | "exclude_media" => {
                config.trace_exclude_media_body = true;
            }
            "full" | "all" => {
                config.trace_exclude_media_body = false;
            }
            _ => {
                return Err(
                    "--trace-filter supports headers-only, media, or full in this build"
                        .to_string(),
                );
            }
        }
    }
    Ok(())
}

pub(super) fn parse_proxy_auth(input: &str) -> Result<String, String> {
    let Some((username, password)) = input.split_once(':') else {
        return Err("--proxy-auth must use user:pass format".to_string());
    };
    if username.is_empty() || password.is_empty() {
        return Err("--proxy-auth username and password must not be empty".to_string());
    }
    if username.chars().any(char::is_control) || password.chars().any(char::is_control) {
        return Err("--proxy-auth must not contain control characters".to_string());
    }
    Ok(input.to_string())
}

pub(super) fn percent_encode(input: &str) -> String {
    let mut out = String::new();
    for byte in input.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}
