use super::*;

pub(super) fn explain_action(item: &ResolvedAction, req: &RequestMeta) -> String {
    let action = &item.action;
    match action {
        Action::Host(pool) => format!(
            "host({})",
            pool.addresses()
                .iter()
                .map(|address| explain_value(address, item, req))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Action::Upstream(value) => format!("upstream({})", explain_value(value, item, req)),
        Action::Mock(Value::Inline(value)) => format!("mock({})", item.render(value, req)),
        Action::Mock(Value::File(value)) => format!("mock(<{}>)", item.render(value, req)),
        Action::Mock(Value::Reference(value)) => format!("mock(@{value})"),
        Action::MockRaw(Value::Inline(value)) => {
            format!("mock.raw({})", item.render(value, req))
        }
        Action::MockRaw(Value::File(value)) => {
            format!("mock.raw(<{}>)", item.render(value, req))
        }
        Action::MockRaw(Value::Reference(value)) => format!("mock.raw(@{value})"),
        Action::MockInline(op) => {
            let mut parts = Vec::new();
            if let Some(status) = op.status {
                parts.push(format!("status={status}"));
            }
            for (name, value) in &op.headers {
                parts.push(format!(
                    "header={name}: {}",
                    explain_value(value, item, req)
                ));
            }
            if let Some(body) = &op.body {
                parts.push(format!("body={}", explain_raw_value(body)));
            }
            format!("mock({})", parts.join(", "))
        }
        Action::MapRemote(value) => format!("map.remote({})", explain_value(value, item, req)),
        Action::Status(code) => format!("status({code})"),
        Action::Redirect { url, code } => {
            format!("redirect({}, {code})", explain_value(url, item, req))
        }
        Action::ReqHeader(op) => format!("req.header({})", explain_header_op(op, item, req)),
        Action::ResHeader(op) => format!("res.header({})", explain_header_op(op, item, req)),
        Action::ReqMethod(method) => format!("req.method({})", explain_value(method, item, req)),
        Action::ReqCookie(op) => format!("req.cookie({})", explain_cookie_op(op, item, req)),
        Action::ResCookie(op) => format!("res.cookie({})", explain_cookie_op(op, item, req)),
        Action::ReqUa(value) => format!("req.ua({})", explain_value(value, item, req)),
        Action::ReqReferer(value) => format!("req.referer({})", explain_value(value, item, req)),
        Action::ReqAuth(value) => format!("req.auth({})", explain_value(value, item, req)),
        Action::ReqForwarded(value) => {
            format!("req.forwarded({})", explain_value(value, item, req))
        }
        Action::ReqType(value) => format!("req.type({})", explain_value(value, item, req)),
        Action::ReqCharset(value) => {
            format!("req.charset({})", explain_value(value, item, req))
        }
        Action::ResCors(op) => format!("res.cors({})", explain_cors_op(op, item, req)),
        Action::ResType(value) => format!("res.type({})", explain_value(value, item, req)),
        Action::ResCharset(value) => {
            format!("res.charset({})", explain_value(value, item, req))
        }
        Action::ResMerge(value) => format!("res.merge({})", explain_value(value, item, req)),
        Action::ResTrailer(op) => format!("res.trailer({})", explain_header_op(op, item, req)),
        Action::Attachment(Some(value)) => {
            format!("attachment({})", explain_value(value, item, req))
        }
        Action::Cache(op) => format!("cache({})", explain_cache_op(op, item, req)),
        Action::Tls(op) => explain_tls_op(op, item, req),
        Action::UrlRewrite { from, to } => {
            let (from, to) = match from {
                UrlRewritePattern::Plain(value) => (
                    explain_value(value, item, req),
                    explain_value(to, item, req),
                ),
                UrlRewritePattern::Regex(pattern) => (pattern.display(), explain_raw_value(to)),
            };
            format!("url.rewrite({from}, {to})")
        }
        Action::UrlQuery(ops) => format!(
            "url.query({})",
            ops.iter()
                .map(|op| match op {
                    QueryOp::Set { name, value } => {
                        format!("{name}={}", explain_value(value, item, req))
                    }
                    QueryOp::Remove { name } => format!("-{name}"),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Action::Delete(operations) => format!(
            "delete({})",
            operations
                .iter()
                .map(explain_delete_op)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Action::ReqBody(op) => format!("req.body.{}", explain_body_op(op, item, req)),
        Action::ResBody(op) => format!("res.body.{}", explain_body_op(op, item, req)),
        Action::Inject(op) => format!(
            "inject({}, {}, {})",
            op.target.as_str(),
            explain_value(&op.value, item, req),
            op.mode.as_str()
        ),
        Action::Direct => "direct".to_string(),
        Action::Bypass => "bypass".to_string(),
        Action::Hide => "hide".to_string(),
        Action::Tag(value) => format!("tag({})", explain_value(value, item, req)),
        Action::Skip(families) if families.is_empty() => "skip()".to_string(),
        Action::Skip(families) => format!("skip({})", families.join(", ")),
        other => format!("{other:?}"),
    }
}

fn explain_delete_op(operation: &DeleteOp) -> String {
    match operation {
        DeleteOp::Pathname => "pathname".to_string(),
        DeleteOp::PathSegment(DeletePathSegment::Index(index)) => {
            format!("pathname.{index}")
        }
        DeleteOp::PathSegment(DeletePathSegment::Last) => "pathname.last".to_string(),
        DeleteOp::UrlParams => "urlParams".to_string(),
        DeleteOp::UrlParam(name) => format!("urlParams.{name}"),
        DeleteOp::ReqHeader(name) => format!("reqHeaders.{name}"),
        DeleteOp::ResHeader(name) => format!("resHeaders.{name}"),
        DeleteOp::ReqBody => "reqBody".to_string(),
        DeleteOp::ResBody => "resBody".to_string(),
        DeleteOp::ReqBodyPath(path) => format!("reqBody.{}", explain_delete_body_path(path)),
        DeleteOp::ResBodyPath(path) => format!("resBody.{}", explain_delete_body_path(path)),
        DeleteOp::ReqType => "reqType".to_string(),
        DeleteOp::ResType => "resType".to_string(),
        DeleteOp::ReqCharset => "reqCharset".to_string(),
        DeleteOp::ResCharset => "resCharset".to_string(),
        DeleteOp::ReqCookie(name) => format!("reqCookies.{name}"),
        DeleteOp::ResCookie(name) => format!("resCookies.{name}"),
        DeleteOp::ReqCookies => "reqCookies".to_string(),
        DeleteOp::ResCookies => "resCookies".to_string(),
        DeleteOp::Trailer(name) => format!("trailer.{name}"),
        DeleteOp::Trailers => "trailers".to_string(),
    }
}

fn explain_delete_body_path(path: &DeleteBodyPath) -> String {
    let mut output = String::new();
    for (index, segment) in path.segments().iter().enumerate() {
        match segment {
            DeleteBodyPathSegment::Key(key) => {
                if index > 0 {
                    output.push('.');
                }
                for ch in key.chars() {
                    match ch {
                        '\\' => output.push_str("\\\\"),
                        '.' | ',' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '#' | '|'
                        | '&' | ' ' | '\'' | '"' => {
                            output.push('\\');
                            output.push(ch);
                        }
                        '\n' => output.push_str("\\n"),
                        '\r' => output.push_str("\\r"),
                        '\t' => output.push_str("\\t"),
                        '\u{000c}' => output.push_str("\\f"),
                        '\u{000b}' => output.push_str("\\v"),
                        other => output.push(other),
                    }
                }
            }
            DeleteBodyPathSegment::Index(value) => output.push_str(&format!("[{value}]")),
        }
    }
    output
}

fn explain_header_op(op: &HeaderOp, item: &ResolvedAction, req: &RequestMeta) -> String {
    match op {
        HeaderOp::Set { name, value } => {
            format!("{name}: {}", explain_value(value, item, req))
        }
        HeaderOp::Remove { name } => format!("-{name}"),
        HeaderOp::Replace {
            name,
            pattern,
            replacement,
        } => format!(
            "{name} ~ /{}/{}",
            pattern.pattern().replace('/', "\\/"),
            replacement.replace('/', "\\/")
        ),
    }
}

fn explain_cookie_op(op: &CookieOp, item: &ResolvedAction, req: &RequestMeta) -> String {
    match op {
        CookieOp::Set { name, value, attrs } => {
            let mut out = format!("{name}={}", explain_value(value, item, req));
            for attr in attrs {
                out.push_str("; ");
                out.push_str(&attr.name);
                if let Some(value) = &attr.value {
                    out.push('=');
                    out.push_str(&explain_value(value, item, req));
                }
            }
            out
        }
        CookieOp::Remove { name } => format!("-{name}"),
    }
}

fn explain_cors_op(op: &CorsOp, item: &ResolvedAction, req: &RequestMeta) -> String {
    let mut args = vec![explain_value(&op.origin, item, req)];
    if let Some(value) = &op.methods {
        args.push(format!("methods={}", explain_value(value, item, req)));
    }
    if let Some(value) = &op.headers {
        args.push(format!("headers={}", explain_value(value, item, req)));
    }
    if let Some(value) = op.credentials {
        args.push(format!("credentials={value}"));
    }
    if let Some(value) = &op.expose {
        args.push(format!("expose={}", explain_value(value, item, req)));
    }
    if let Some(value) = &op.max_age {
        args.push(format!("max-age={}", explain_value(value, item, req)));
    }
    args.join(", ")
}

fn explain_cache_op(op: &CacheOp, item: &ResolvedAction, req: &RequestMeta) -> String {
    match op {
        CacheOp::Off => "off".to_string(),
        CacheOp::Directives(directives) => directives
            .iter()
            .map(|directive| match &directive.value {
                Some(value) => format!("{}={}", directive.name, explain_value(value, item, req)),
                None => directive.name.clone(),
            })
            .collect::<Vec<_>>()
            .join(", "),
    }
}

fn explain_tls_op(op: &TlsOp, item: &ResolvedAction, req: &RequestMeta) -> String {
    let mut parts = Vec::new();
    if let (Some(cert), Some(key)) = (&op.client_cert, &op.client_key) {
        parts.push(format!("client-cert={}", item.render(cert, req)));
        parts.push(format!("client-key={}", item.render(key, req)));
    }
    if let Some(min_version) = op.min_version {
        parts.push(format!("min={}", min_version.as_str()));
    }
    if !op.ciphers.is_empty() {
        parts.push(format!(
            "ciphers={}",
            op.ciphers
                .iter()
                .map(|cipher| cipher.as_str())
                .collect::<Vec<_>>()
                .join(":")
        ));
    }
    format!("tls({})", parts.join(", "))
}

fn explain_body_op(op: &BodyOp, item: &ResolvedAction, req: &RequestMeta) -> String {
    match op {
        BodyOp::Set(Value::Inline(value)) => format!("set({})", item.render(value, req)),
        BodyOp::Prepend(Value::Inline(value)) => {
            format!("prepend({})", item.render(value, req))
        }
        BodyOp::Append(Value::Inline(value)) => format!("append({})", item.render(value, req)),
        BodyOp::Set(Value::File(value)) => format!("set(<{}>)", item.render(value, req)),
        BodyOp::Prepend(Value::File(value)) => {
            format!("prepend(<{}>)", item.render(value, req))
        }
        BodyOp::Append(Value::File(value)) => format!("append(<{}>)", item.render(value, req)),
        BodyOp::Set(Value::Reference(value)) => format!("set(@{value})"),
        BodyOp::Prepend(Value::Reference(value)) => format!("prepend(@{value})"),
        BodyOp::Append(Value::Reference(value)) => format!("append(@{value})"),
        BodyOp::Replace {
            pattern,
            replacement,
        } => format!("replace({}, {})", pattern.display(), replacement),
    }
}

fn explain_value(value: &Value, item: &ResolvedAction, req: &RequestMeta) -> String {
    match value {
        Value::Inline(value) => item.render(value, req),
        Value::File(_) | Value::Reference(_) => explain_raw_value(value),
    }
}

fn explain_raw_value(value: &Value) -> String {
    match value {
        Value::Inline(value) => value.clone(),
        Value::File(value) => format!("<{value}>"),
        Value::Reference(value) => format!("@{value}"),
    }
}
