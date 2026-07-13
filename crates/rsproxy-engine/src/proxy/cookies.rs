use super::*;

pub(super) fn apply_req_cookie(
    headers: &mut Vec<(String, String)>,
    op: &CookieOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    let mut cookies = http::header(headers, "cookie")
        .map(parse_cookie_header)
        .unwrap_or_default();
    match op {
        CookieOp::Set { name, value, .. } => {
            let value = resolve_value_text(value, item, meta, state)?;
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
        http::set_header(
            headers,
            "Cookie",
            cookies
                .into_iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        );
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
    match op {
        CookieOp::Set { name, value, attrs } => {
            let value = resolve_value_text(value, item, meta, state)?;
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
    let mut out = format!("{name}={value}");
    if attrs.is_empty() {
        out.push_str("; Path=/");
        return Ok(out);
    }
    for attr in attrs {
        out.push_str("; ");
        out.push_str(&attr.name);
        if let Some(value) = &attr.value {
            out.push('=');
            out.push_str(&resolve_value_text(value, item, meta, state)?);
        }
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
