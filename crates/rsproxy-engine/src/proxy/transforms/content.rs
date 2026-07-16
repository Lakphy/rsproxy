use super::*;

pub(in crate::proxy) fn apply_url_actions(
    full_url: &str,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
) -> io::Result<String> {
    let mut url =
        UrlParts::parse(full_url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    for item in actions {
        match &item.action {
            Action::MapRemote(value) => {
                let rendered = resolve_value_text(value, item, meta, state)?;
                let target = UrlParts::parse(rendered.trim()).map_err(|e| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("map.remote target `{rendered}`: {e}"),
                    )
                })?;
                if target.scheme != "http" && target.scheme != "https" {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("map.remote target must use http or https: {rendered}"),
                    ));
                }
                // A target without an explicit path keeps the original path
                // and query; an explicit path (even a bare `/`) replaces both.
                let explicit_path = rendered
                    .trim()
                    .split_once("://")
                    .is_some_and(|(_, rest)| rest.contains('/') || rest.contains('?'));
                url.scheme = target.scheme;
                url.host = target.host;
                url.port = target.port;
                if explicit_path {
                    url.path = target.path;
                    url.query = target.query;
                }
            }
            Action::UrlRewrite { from, to } => {
                let origin = url.origin_form();
                let rewritten = match from {
                    UrlRewritePattern::Plain(from) => {
                        let from = resolve_value_text(from, item, meta, state)?;
                        let to = resolve_value_text(to, item, meta, state)?;
                        origin.replace(&from, &to)
                    }
                    UrlRewritePattern::Regex(pattern) => {
                        let replacement = resolve_raw_value_text(to, item, meta, state)?;
                        pattern.replace_all(&origin, &replacement)
                    }
                };
                set_url_origin(&mut url, &rewritten);
            }
            Action::UrlQuery(ops) => {
                let mut pairs = parse_query_pairs(url.query.as_deref().unwrap_or(""));
                for op in ops {
                    match op {
                        QueryOp::Set { name, value } => {
                            let rendered = resolve_value_text(value, item, meta, state)?;
                            if let Some((_, existing)) = pairs.iter_mut().find(|(k, _)| k == name) {
                                *existing = rendered;
                            } else {
                                pairs.push((name.clone(), rendered));
                            }
                        }
                        QueryOp::Remove { name } => pairs.retain(|(k, _)| k != name),
                    }
                }
                url.query = if pairs.is_empty() {
                    None
                } else {
                    Some(
                        pairs
                            .into_iter()
                            .map(|(k, v)| format!("{k}={v}"))
                            .collect::<Vec<_>>()
                            .join("&"),
                    )
                };
            }
            Action::Delete(operations) => apply_url_delete(&mut url, operations),
            _ => {}
        }
    }
    Ok(url_to_string(&url))
}

pub(in crate::proxy) fn apply_body_op(
    body: &mut Vec<u8>,
    op: &BodyOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    match op {
        BodyOp::Set(value) => *body = resolve_value_bytes(value, item, meta, state)?,
        BodyOp::Prepend(value) => {
            let mut next = resolve_value_bytes(value, item, meta, state)?;
            next.extend_from_slice(body);
            *body = next;
        }
        BodyOp::Append(value) => {
            body.extend_from_slice(&resolve_value_bytes(value, item, meta, state)?)
        }
        BodyOp::Replace {
            pattern,
            replacement,
        } => {
            if let Ok(text) = std::str::from_utf8(body) {
                *body = pattern.replace_all(text, replacement).into_bytes();
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
    match op.mode {
        InjectMode::Append => {
            body.extend_from_slice(&resolve_value_bytes(&op.value, item, meta, state)?)
        }
        InjectMode::Prepend => {
            let mut next = resolve_value_bytes(&op.value, item, meta, state)?;
            next.extend_from_slice(body);
            *body = next;
        }
        InjectMode::Replace => *body = resolve_value_bytes(&op.value, item, meta, state)?,
    }
    Ok(())
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
