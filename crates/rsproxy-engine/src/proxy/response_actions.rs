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
                    resolve_value_text(value, item, meta, state)?,
                );
            }
            Action::ResCharset(value) => {
                set_charset(headers, &resolve_value_text(value, item, meta, state)?)
            }
            Action::ResMerge(value) if include_body => {
                let patch = resolve_value_text(value, item, meta, state)?;
                apply_res_merge(body, &patch)?;
            }
            Action::Attachment(filename) => {
                let value = filename
                    .as_ref()
                    .map(|name| {
                        resolve_value_text(name, item, meta, state)
                            .map(|name| format!("attachment; filename=\"{name}\""))
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
    Ok(())
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
    Ok(())
}

pub(super) fn apply_res_cors(
    headers: &mut Vec<(String, String)>,
    op: &CorsOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    let origin = resolve_value_text(&op.origin, item, meta, state)?;
    http::set_header(headers, "Access-Control-Allow-Origin", origin.clone());
    http::set_header(
        headers,
        "Access-Control-Allow-Methods",
        op.methods
            .as_ref()
            .map(|value| resolve_value_text(value, item, meta, state))
            .transpose()?
            .unwrap_or_else(|| "GET,POST,PUT,PATCH,DELETE,OPTIONS".to_string()),
    );
    http::set_header(
        headers,
        "Access-Control-Allow-Headers",
        op.headers
            .as_ref()
            .map(|value| resolve_value_text(value, item, meta, state))
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
            resolve_value_text(value, item, meta, state)?,
        );
    }
    if let Some(value) = &op.max_age {
        http::set_header(
            headers,
            "Access-Control-Max-Age",
            resolve_value_text(value, item, meta, state)?,
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
    directives
        .iter()
        .map(|directive| match &directive.value {
            Some(value) => Ok(format!(
                "{}={}",
                directive.name,
                resolve_value_text(value, item, meta, state)?
            )),
            None => Ok(directive.name.clone()),
        })
        .collect::<io::Result<Vec<_>>>()
        .map(|values| values.join(", "))
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

pub(super) fn apply_res_merge(body: &mut Vec<u8>, patch: &str) -> io::Result<bool> {
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
    *body = serde_json::to_vec(&base).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("serialize res.merge json: {err}"),
        )
    })?;
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
