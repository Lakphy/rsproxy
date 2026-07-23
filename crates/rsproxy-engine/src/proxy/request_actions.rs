use super::*;

pub(super) fn apply_request_actions(
    req: &mut RawRequest,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
) -> io::Result<()> {
    apply_request_actions_inner(req, meta, actions, state, true)
}

pub(super) fn apply_streaming_request_actions(
    req: &mut RawRequest,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
) -> io::Result<()> {
    apply_request_actions_inner(req, meta, actions, state, false)
}

fn apply_request_actions_inner(
    req: &mut RawRequest,
    meta: &RequestMeta,
    actions: &[ResolvedAction],
    state: &SharedState,
    body_available: bool,
) -> io::Result<()> {
    let mut body_changed = false;
    let header_limit = rule_header_limit(state);
    for item in actions {
        match &item.action {
            Action::ReqHeader(op) => apply_header_op(&mut req.headers, op, item, meta, state)?,
            Action::ReqMethod(method) => {
                req.method = resolve_value_text_bounded(method, item, meta, state, header_limit)?
                    .to_ascii_uppercase()
            }
            Action::ReqCookie(op) => apply_req_cookie(&mut req.headers, op, item, meta, state)?,
            Action::ReqUa(value) => {
                http::set_header(
                    &mut req.headers,
                    "User-Agent",
                    resolve_value_text_bounded(value, item, meta, state, header_limit)?,
                );
            }
            Action::ReqReferer(value) => {
                http::set_header(
                    &mut req.headers,
                    "Referer",
                    resolve_value_text_bounded(value, item, meta, state, header_limit)?,
                );
            }
            Action::ReqAuth(value) => {
                let value = resolve_value_text_bounded(value, item, meta, state, header_limit)?;
                http::set_header(
                    &mut req.headers,
                    "Authorization",
                    format!("Basic {}", base64(value.as_bytes())),
                );
            }
            Action::ReqForwarded(value) => {
                let value = forwarded_for_value(&resolve_value_text_bounded(
                    value,
                    item,
                    meta,
                    state,
                    header_limit,
                )?);
                http::set_header(&mut req.headers, "X-Forwarded-For", value);
            }
            Action::ReqType(value) => {
                http::set_header(
                    &mut req.headers,
                    "Content-Type",
                    resolve_value_text_bounded(value, item, meta, state, header_limit)?,
                );
            }
            Action::ReqCharset(value) => {
                set_charset(
                    &mut req.headers,
                    &resolve_value_text_bounded(value, item, meta, state, header_limit)?,
                );
            }
            Action::ReqBody(op) if body_available => {
                apply_body_op(&mut req.body, op, item, meta, state)?;
                body_changed = true;
            }
            Action::Delete(operations) => {
                body_changed |= apply_request_delete(req, operations, body_available);
            }
            _ => {}
        }
    }
    if body_available
        && (body_changed
            || !req.body.is_empty()
            || http::header(&req.headers, "content-length").is_some())
    {
        update_body_headers(&mut req.headers, req.body.len());
    }
    validate_http_method(&req.method)?;
    validate_header_block(&req.headers, state)?;
    Ok(())
}

pub(super) fn forwarded_for_value(value: &str) -> String {
    value
        .parse::<std::net::SocketAddr>()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| value.to_string())
}
