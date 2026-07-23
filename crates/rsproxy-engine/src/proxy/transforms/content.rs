use super::*;

pub(in crate::proxy) fn apply_url_actions(
    full_url: &str,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
) -> io::Result<String> {
    let url_limit = state
        .config
        .max_header_size
        .min(rsproxy_rules::MAX_RULE_RENDERED_VALUE_BYTES);
    let mut url =
        UrlParts::parse(full_url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    for item in actions {
        match &item.action {
            Action::MapRemote(value) => {
                let rendered = resolve_value_text_bounded(value, item, meta, state, url_limit)?;
                let target = UrlParts::parse(rendered.trim()).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("map.remote target `{rendered}`: {e}"),
                    )
                })?;
                let target_scheme = match target.scheme.as_str() {
                    "http" => "http",
                    "https" => "https",
                    "ws" if is_websocket_request(&meta.headers) => "http",
                    "wss" if is_websocket_request(&meta.headers) => "https",
                    "ws" | "wss" => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!(
                                "map.remote WebSocket target requires an Upgrade request: {rendered}"
                            ),
                        ));
                    }
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidInput,
                            format!(
                                "map.remote target must use http, https, ws, or wss: {rendered}"
                            ),
                        ));
                    }
                };
                // A target without an explicit path keeps the original path
                // and query; an explicit path (even a bare `/`) replaces both.
                let explicit_path = rendered
                    .trim()
                    .split_once("://")
                    .is_some_and(|(_, rest)| rest.contains('/') || rest.contains('?'));
                url.scheme = target_scheme.to_string();
                url.host = target.host;
                url.port = target.port;
                if explicit_path {
                    url.path = target.path;
                    url.query = target.query;
                }
                validate_rewritten_url(&url_to_string(&url), url_limit)?;
            }
            Action::UrlRewrite { from, to } => {
                let origin = url.origin_form();
                let rewritten = match from {
                    UrlRewritePattern::Plain(from) => {
                        let from = resolve_value_text_bounded(from, item, meta, state, url_limit)?;
                        let to = resolve_value_text_bounded(to, item, meta, state, url_limit)?;
                        replace_plain_bounded(&origin, &from, &to, url_limit, "rewritten URL")?
                    }
                    UrlRewritePattern::Regex(pattern) => {
                        let replacement = resolve_raw_value_text(to, item, meta, state)?;
                        pattern
                            .replace_all_bounded(&origin, &replacement, url_limit)
                            .map_err(rule_output_error)?
                    }
                };
                set_url_origin(&mut url, &rewritten);
            }
            Action::UrlQuery(ops) => {
                let mut pairs = parse_query_pairs(url.query.as_deref().unwrap_or(""));
                for op in ops {
                    match op {
                        QueryOp::Set { name, value } => {
                            let rendered =
                                resolve_value_text_bounded(value, item, meta, state, url_limit)?;
                            if let Some((_, existing)) = pairs.iter_mut().find(|(k, _)| k == name) {
                                *existing = rendered;
                            } else {
                                pairs.push((name.clone(), rendered));
                            }
                        }
                        QueryOp::Remove { name } => pairs.retain(|(k, _)| k != name),
                    }
                    query_pairs_length(&pairs, url_limit)?;
                }
                url.query = if pairs.is_empty() {
                    None
                } else {
                    Some(render_query_pairs(&pairs, url_limit)?)
                };
            }
            Action::Delete(operations) => apply_url_delete(&mut url, operations),
            _ => {}
        }
    }
    let output = url_to_string(&url);
    validate_rewritten_url(&output, url_limit)?;
    Ok(output)
}

pub(in crate::proxy) fn apply_body_op(
    body: &mut Vec<u8>,
    op: &BodyOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    let limit = rule_body_limit(state);
    match op {
        BodyOp::Set(value) => *body = resolve_value_bytes_bounded(value, item, meta, state, limit)?,
        BodyOp::Prepend(value) => {
            let remaining = remaining_capacity(body.len(), limit, "body transformation")?;
            let mut next = resolve_value_bytes_bounded(value, item, meta, state, remaining)?;
            next.extend_from_slice(body);
            *body = next;
        }
        BodyOp::Append(value) => {
            let remaining = remaining_capacity(body.len(), limit, "body transformation")?;
            body.extend_from_slice(&resolve_value_bytes_bounded(
                value, item, meta, state, remaining,
            )?)
        }
        BodyOp::Replace {
            pattern,
            replacement,
        } => {
            if let Ok(text) = std::str::from_utf8(body) {
                *body = pattern
                    .replace_all_bounded(text, replacement, limit)
                    .map_err(rule_output_error)?
                    .into_bytes();
            }
        }
    }
    Ok(())
}

pub(in crate::proxy) fn apply_inject_op(
    headers: &[(String, String)],
    body: &mut Vec<u8>,
    op: &InjectOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    if !inject_content_type_matches(headers, op.target) {
        return Ok(());
    }
    let limit = rule_body_limit(state);
    match op.mode {
        InjectMode::Append => {
            let remaining = remaining_capacity(body.len(), limit, "body injection")?;
            body.extend_from_slice(&resolve_value_bytes_bounded(
                &op.value, item, meta, state, remaining,
            )?)
        }
        InjectMode::Prepend => {
            let remaining = remaining_capacity(body.len(), limit, "body injection")?;
            let mut next = resolve_value_bytes_bounded(&op.value, item, meta, state, remaining)?;
            next.extend_from_slice(body);
            *body = next;
        }
        InjectMode::Replace => {
            *body = resolve_value_bytes_bounded(&op.value, item, meta, state, limit)?
        }
    }
    Ok(())
}

fn replace_plain_bounded(
    input: &str,
    from: &str,
    to: &str,
    limit: usize,
    label: &str,
) -> io::Result<String> {
    let matches = input.match_indices(from).count();
    let removed = matches
        .checked_mul(from.len())
        .ok_or_else(|| output_limit_error(label, limit))?;
    let inserted = matches
        .checked_mul(to.len())
        .ok_or_else(|| output_limit_error(label, limit))?;
    let length = input
        .len()
        .checked_sub(removed)
        .and_then(|length| length.checked_add(inserted))
        .ok_or_else(|| output_limit_error(label, limit))?;
    if length > limit {
        return Err(output_limit_error(label, limit));
    }
    Ok(input.replace(from, to))
}

fn render_query_pairs(pairs: &[(String, String)], limit: usize) -> io::Result<String> {
    let length = query_pairs_length(pairs, limit)?;
    let mut output = String::with_capacity(length);
    for (index, (name, value)) in pairs.iter().enumerate() {
        if index > 0 {
            output.push('&');
        }
        output.push_str(name);
        output.push('=');
        output.push_str(value);
    }
    Ok(output)
}

fn query_pairs_length(pairs: &[(String, String)], limit: usize) -> io::Result<usize> {
    let mut length = pairs.len().saturating_sub(1);
    for (name, value) in pairs {
        length = length
            .checked_add(name.len())
            .and_then(|length| length.checked_add(1))
            .and_then(|length| length.checked_add(value.len()))
            .ok_or_else(|| output_limit_error("query string", limit))?;
        if length > limit {
            return Err(output_limit_error("query string", limit));
        }
    }
    Ok(length)
}

pub(in crate::proxy) fn remaining_capacity(
    current: usize,
    limit: usize,
    label: &str,
) -> io::Result<usize> {
    limit
        .checked_sub(current)
        .ok_or_else(|| output_limit_error(label, limit))
}

pub(in crate::proxy) fn rule_header_limit(state: &SharedState) -> usize {
    state
        .config
        .max_header_size
        .min(rsproxy_rules::MAX_RULE_RENDERED_VALUE_BYTES)
}

pub(in crate::proxy) fn rule_body_limit(state: &SharedState) -> usize {
    state
        .config
        .body_buffer_limit
        .min(rsproxy_rules::MAX_RULE_RENDERED_VALUE_BYTES)
}

pub(in crate::proxy) fn ensure_output_limit(
    value: &str,
    limit: usize,
    label: &str,
) -> io::Result<()> {
    if value.len() > limit {
        return Err(output_limit_error(label, limit));
    }
    Ok(())
}

fn validate_rewritten_url(value: &str, limit: usize) -> io::Result<()> {
    ensure_output_limit(value, limit, "rewritten URL")?;
    if value.bytes().any(|byte| byte <= b' ' || byte == 0x7f) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "rewritten URL contains whitespace or an ASCII control character",
        ));
    }
    Ok(())
}

pub(in crate::proxy) fn push_text_bounded(
    output: &mut String,
    value: &str,
    limit: usize,
    label: &str,
) -> io::Result<()> {
    if output
        .len()
        .checked_add(value.len())
        .is_none_or(|length| length > limit)
    {
        return Err(output_limit_error(label, limit));
    }
    output.push_str(value);
    Ok(())
}

pub(in crate::proxy) fn output_limit_error(label: &str, limit: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{label} exceeds the {limit}-byte output limit"),
    )
}

fn rule_output_error(error: rsproxy_rules::RuleModelError) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error)
}

pub(in crate::proxy) fn inject_content_type_matches(
    headers: &[(String, String)],
    target: InjectTarget,
) -> bool {
    let Some(content_type) = http::header(headers, "content-type") else {
        return false;
    };
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    match target {
        InjectTarget::Html => matches!(mime.as_str(), "text/html" | "application/xhtml+xml"),
        InjectTarget::Js => matches!(
            mime.as_str(),
            "application/javascript" | "text/javascript" | "application/x-javascript"
        ),
        InjectTarget::Css => mime == "text/css",
    }
}

pub(in crate::proxy) fn trace_body_limit_for_headers(
    config: &ProxyConfig,
    headers: &[(String, String)],
) -> usize {
    if config.trace_body_limit == 0 {
        return 0;
    }
    if config.trace_exclude_media_body && trace_media_content_type(headers) {
        0
    } else {
        config.trace_body_limit
    }
}

pub(in crate::proxy) fn trace_media_content_type(headers: &[(String, String)]) -> bool {
    let Some(content_type) = http::header(headers, "content-type") else {
        return false;
    };
    let mime = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    mime.starts_with("image/")
        || mime.starts_with("audio/")
        || mime.starts_with("video/")
        || mime.starts_with("font/")
        || mime.starts_with("application/font-")
        || mime.starts_with("application/x-font-")
        || matches!(mime.as_str(), "application/vnd.ms-fontobject")
}

pub(in crate::proxy) fn update_body_headers(headers: &mut Vec<(String, String)>, len: usize) {
    http::remove_header(headers, "transfer-encoding");
    http::remove_header(headers, "trailer");
    http::set_header(headers, "Content-Length", len.to_string());
}
