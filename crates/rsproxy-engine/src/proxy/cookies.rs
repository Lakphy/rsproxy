use super::*;

pub(super) fn apply_req_cookie(
    headers: &mut Vec<(String, String)>,
    op: &CookieOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    let limit = rule_header_limit(state);
    let mut cookies = http::header(headers, "cookie")
        .map(parse_cookie_header)
        .unwrap_or_default();
    match op {
        CookieOp::Set { name, value, .. } => {
            let value = resolve_value_text_bounded(value, item, meta, state, limit)?;
            if let Some((_, existing)) = cookies.iter_mut().find(|(key, _)| key == name) {
                *existing = value;
            } else {
                cookies.push((name.clone(), value));
            }
        }
        CookieOp::Remove { name } => cookies.retain(|(key, _)| key != name),
    }
    if cookies.is_empty() {
        http::remove_header(headers, "cookie");
    } else {
        http::set_header(headers, "Cookie", render_request_cookies(&cookies, limit)?);
    }
    Ok(())
}

pub(super) fn apply_res_cookie(
    headers: &mut Vec<(String, String)>,
    op: &CookieOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    let limit = rule_header_limit(state);
    match op {
        CookieOp::Set { name, value, attrs } => {
            let value = resolve_value_text_bounded(value, item, meta, state, limit)?;
            headers.push((
                "Set-Cookie".to_string(),
                render_set_cookie(name, &value, attrs, item, meta, state)?,
            ));
        }
        CookieOp::Remove { name } => headers.retain(|(key, value)| {
            !(key.eq_ignore_ascii_case("set-cookie")
                && value
                    .split_once('=')
                    .map(|(cookie_name, _)| cookie_name.trim() == name)
                    .unwrap_or(false))
        }),
    }
    Ok(())
}

pub(super) fn render_set_cookie(
    name: &str,
    value: &str,
    attrs: &[rsproxy_rules::CookieAttr],
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<String> {
    let limit = rule_header_limit(state);
    let mut out = String::new();
    push_text_bounded(&mut out, name, limit, "Set-Cookie value")?;
    push_text_bounded(&mut out, "=", limit, "Set-Cookie value")?;
    push_text_bounded(&mut out, value, limit, "Set-Cookie value")?;
    if attrs.is_empty() {
        push_text_bounded(&mut out, "; Path=/", limit, "Set-Cookie value")?;
        return Ok(out);
    }
    for attr in attrs {
        push_text_bounded(&mut out, "; ", limit, "Set-Cookie value")?;
        push_text_bounded(&mut out, &attr.name, limit, "Set-Cookie value")?;
        if let Some(value) = &attr.value {
            push_text_bounded(&mut out, "=", limit, "Set-Cookie value")?;
            let remaining = remaining_capacity(out.len(), limit, "Set-Cookie value")?;
            let value = resolve_value_text_bounded(value, item, meta, state, remaining)?;
            push_text_bounded(&mut out, &value, limit, "Set-Cookie value")?;
        }
    }
    Ok(out)
}

fn render_request_cookies(cookies: &[(String, String)], limit: usize) -> io::Result<String> {
    let mut out = String::new();
    for (index, (name, value)) in cookies.iter().enumerate() {
        if index > 0 {
            push_text_bounded(&mut out, "; ", limit, "Cookie header")?;
        }
        push_text_bounded(&mut out, name, limit, "Cookie header")?;
        push_text_bounded(&mut out, "=", limit, "Cookie header")?;
        push_text_bounded(&mut out, value, limit, "Cookie header")?;
    }
    Ok(out)
}

pub(super) fn parse_cookie_header(input: &str) -> Vec<(String, String)> {
    input
        .split(';')
        .filter_map(|part| {
            let (key, value) = part.trim().split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

pub(super) fn set_charset(headers: &mut Vec<(String, String)>, charset: &str) {
    let content_type = http::header(headers, "content-type")
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("text/plain")
                .trim()
                .to_string()
        })
        .unwrap_or_else(|| "text/plain".to_string());
    http::set_header(
        headers,
        "Content-Type",
        format!("{content_type}; charset={charset}"),
    );
}

pub(super) fn set_url_origin(url: &mut UrlParts, origin: &str) {
    let origin = if origin.starts_with('/') {
        origin.to_string()
    } else {
        format!("/{origin}")
    };
    let (path, query) = origin
        .split_once('?')
        .map(|(path, query)| (path.to_string(), Some(query.to_string())))
        .unwrap_or((origin, None));
    url.path = if path.is_empty() {
        "/".to_string()
    } else {
        path
    };
    url.query = query;
}
