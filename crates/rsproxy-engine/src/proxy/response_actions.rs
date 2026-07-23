use super::*;

pub(super) fn apply_response_actions(
    head: &mut http::RawResponseHead,
    headers: &mut Vec<(String, String)>,
    body: &mut Vec<u8>,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
) -> io::Result<()> {
    apply_response_actions_inner(head, headers, body, meta, actions, state, true)
}

pub(super) fn apply_streaming_response_actions(
    head: &mut http::RawResponseHead,
    headers: &mut Vec<(String, String)>,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
) -> io::Result<()> {
    apply_response_actions_inner(head, headers, &mut Vec::new(), meta, actions, state, false)
}

pub(super) fn response_actions_require_body(actions: &[ResolvedAction]) -> bool {
    actions.iter().any(|item| {
        matches!(
            item.action,
            Action::ResMerge(_) | Action::Inject(_) | Action::ResBody(_)
        ) || matches!(
            &item.action,
            Action::Delete(operations)
                if operations.iter().any(|operation| {
                    matches!(operation, DeleteOp::ResBody | DeleteOp::ResBodyPath(_))
                })
        )
    })
}

fn apply_response_actions_inner(
    head: &mut http::RawResponseHead,
    headers: &mut Vec<(String, String)>,
    body: &mut Vec<u8>,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
    include_body: bool,
) -> io::Result<()> {
    let header_limit = rule_header_limit(state);
    for item in actions {
        match &item.action {
            Action::ResHeader(op) => apply_header_op(headers, op, item, meta, state)?,
            Action::ResStatus(code) => {
                head.status = *code;
                head.reason = http::reason_phrase(*code).to_string();
            }
            Action::ResCookie(op) => apply_res_cookie(headers, op, item, meta, state)?,
            Action::ResCors(op) => apply_res_cors(headers, op, item, meta, state)?,
            Action::ResType(value) => {
                http::set_header(
                    headers,
                    "Content-Type",
                    resolve_value_text_bounded(value, item, meta, state, header_limit)?,
                );
            }
            Action::ResCharset(value) => set_charset(
                headers,
                &resolve_value_text_bounded(value, item, meta, state, header_limit)?,
            ),
            Action::ResMerge(value) if include_body => {
                let patch =
                    resolve_value_text_bounded(value, item, meta, state, rule_body_limit(state))?;
                apply_res_merge(body, &patch, rule_body_limit(state))?;
            }
            Action::Attachment(filename) => {
                let value = filename
                    .as_ref()
                    .map(|name| {
                        resolve_value_text_bounded(name, item, meta, state, header_limit)
                            .and_then(|name| content_disposition_attachment(&name))
                    })
                    .transpose()?
                    .unwrap_or_else(|| "attachment".to_string());
                http::set_header(headers, "Content-Disposition", value);
            }
            Action::Cache(CacheOp::Off) => {
                http::set_header(headers, "Cache-Control", "no-store".to_string());
                http::set_header(headers, "Pragma", "no-cache".to_string());
            }
            Action::Cache(CacheOp::Directives(directives)) => {
                http::remove_header(headers, "Pragma");
                http::set_header(
                    headers,
                    "Cache-Control",
                    render_cache_directives(directives, item, meta, state)?,
                );
            }
            Action::Inject(op) if include_body => {
                apply_inject_op(headers, body, op, item, meta, state)?
            }
            Action::ResBody(op) if include_body => apply_body_op(body, op, item, meta, state)?,
            Action::Delete(operations) => {
                apply_response_delete(headers, body, operations, include_body)
            }
            _ => {}
        }
    }
    validate_header_block(headers, state)?;
    Ok(())
}

fn content_disposition_attachment(filename: &str) -> io::Result<String> {
    if filename.is_empty() || filename.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "attachment filename must be non-empty and contain no control characters",
        ));
    }
    let mut fallback = String::with_capacity(filename.len());
    for character in filename.chars() {
        match character {
            '"' | '\\' => {
                fallback.push('\\');
                fallback.push(character);
            }
            character if character.is_ascii() => fallback.push(character),
            _ => fallback.push('_'),
        }
    }
    let mut value = format!("attachment; filename=\"{fallback}\"");
    if !filename.is_ascii() {
        value.push_str("; filename*=UTF-8''");
        for byte in filename.bytes() {
            if byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'&'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
            {
                value.push(char::from(byte));
            } else {
                use std::fmt::Write as _;
                write!(&mut value, "%{byte:02X}").expect("formatting into a String cannot fail");
            }
        }
    }
    Ok(value)
}

pub(super) fn apply_response_trailer_actions(
    trailers: &mut Vec<(String, String)>,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
) -> io::Result<()> {
    for item in actions {
        match &item.action {
            Action::ResTrailer(op) => apply_header_op(trailers, op, item, meta, state)?,
            Action::Delete(operations) => apply_trailer_delete(trailers, operations),
            _ => {}
        }
    }
    validate_trailer_block(trailers, state)?;
    Ok(())
}

pub(super) fn sanitize_upstream_trailers(
    trailers: &mut Vec<(String, String)>,
    response_headers: &[(String, String)],
) -> bool {
    let connection_names = response_headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    let before = trailers.len();
    trailers.retain(|(name, _)| {
        !rsproxy_http::is_forbidden_trailer_name(name)
            && !connection_names
                .iter()
                .any(|connection| name.eq_ignore_ascii_case(connection))
    });
    trailers.len() != before
}

fn validate_trailer_block(trailers: &[(String, String)], state: &SharedState) -> io::Result<()> {
    validate_header_block(trailers, state)?;
    if let Some((name, _)) = trailers
        .iter()
        .find(|(name, _)| rsproxy_http::is_forbidden_trailer_name(name))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("HTTP field `{name}` is forbidden in a trailer section"),
        ));
    }
    Ok(())
}

pub(super) fn apply_res_cors(
    headers: &mut Vec<(String, String)>,
    op: &CorsOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    let limit = rule_header_limit(state);
    let origin = resolve_value_text_bounded(&op.origin, item, meta, state, limit)?;
    http::set_header(headers, "Access-Control-Allow-Origin", origin.clone());
    http::set_header(
        headers,
        "Access-Control-Allow-Methods",
        op.methods
            .as_ref()
            .map(|value| resolve_value_text_bounded(value, item, meta, state, limit))
            .transpose()?
            .unwrap_or_else(|| "GET,POST,PUT,PATCH,DELETE,OPTIONS".to_string()),
    );
    http::set_header(
        headers,
        "Access-Control-Allow-Headers",
        op.headers
            .as_ref()
            .map(|value| resolve_value_text_bounded(value, item, meta, state, limit))
            .transpose()?
            .unwrap_or_else(|| "Content-Type,Authorization,*".to_string()),
    );
    match op.credentials {
        Some(true) => http::set_header(
            headers,
            "Access-Control-Allow-Credentials",
            "true".to_string(),
        ),
        Some(false) => http::remove_header(headers, "Access-Control-Allow-Credentials"),
        None => {}
    }
    if let Some(value) = &op.expose {
        http::set_header(
            headers,
            "Access-Control-Expose-Headers",
            resolve_value_text_bounded(value, item, meta, state, limit)?,
        );
    }
    if let Some(value) = &op.max_age {
        http::set_header(
            headers,
            "Access-Control-Max-Age",
            resolve_value_text_bounded(value, item, meta, state, limit)?,
        );
    }
    if origin != "*" {
        ensure_vary_origin(headers);
    }
    Ok(())
}

pub(super) fn render_cache_directives(
    directives: &[rsproxy_rules::CacheDirective],
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<String> {
    let limit = rule_header_limit(state);
    let mut out = String::new();
    for (index, directive) in directives.iter().enumerate() {
        if index > 0 {
            push_text_bounded(&mut out, ", ", limit, "Cache-Control value")?;
        }
        push_text_bounded(&mut out, &directive.name, limit, "Cache-Control value")?;
        if let Some(value) = &directive.value {
            push_text_bounded(&mut out, "=", limit, "Cache-Control value")?;
            let remaining = remaining_capacity(out.len(), limit, "Cache-Control value")?;
            let value = resolve_value_text_bounded(value, item, meta, state, remaining)?;
            push_text_bounded(&mut out, &value, limit, "Cache-Control value")?;
        }
    }
    Ok(out)
}

pub(super) fn ensure_vary_origin(headers: &mut Vec<(String, String)>) {
    if let Some((_, value)) = headers
        .iter_mut()
        .find(|(name, _)| name.eq_ignore_ascii_case("vary"))
    {
        let has_origin = value
            .split(',')
            .any(|part| part.trim().eq_ignore_ascii_case("origin"));
        if !has_origin {
            if !value.trim().is_empty() {
                value.push_str(", ");
            }
            value.push_str("Origin");
        }
    } else {
        http::set_header(headers, "Vary", "Origin".to_string());
    }
}

pub(super) fn apply_res_merge(body: &mut Vec<u8>, patch: &str, limit: usize) -> io::Result<bool> {
    let Ok(text) = std::str::from_utf8(body) else {
        return Ok(false);
    };
    let mut base = match serde_json::from_str::<JsonValue>(text) {
        Ok(value) if value.is_object() => value,
        Ok(_) | Err(_) => return Ok(false),
    };
    let patch = serde_json::from_str::<JsonValue>(patch).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid res.merge json: {err}"),
        )
    })?;
    if !patch.is_object() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "res.merge json must be an object",
        ));
    }
    merge_json(&mut base, patch);
    let merged = serde_json::to_vec(&base).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("serialize res.merge json: {err}"),
        )
    })?;
    if merged.len() > limit {
        return Err(output_limit_error("merged response body", limit));
    }
    *body = merged;
    Ok(true)
}

pub(super) fn merge_json(base: &mut JsonValue, patch: JsonValue) {
    match (base, patch) {
        (JsonValue::Object(base), JsonValue::Object(patch)) => {
            for (key, value) in patch {
                match base.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base.insert(key, value);
                    }
                }
            }
        }
        (base, patch) => *base = patch,
    }
}
