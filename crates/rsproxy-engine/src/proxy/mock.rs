use super::*;

#[derive(Clone, Debug)]
pub(super) struct MockResponse {
    pub(super) status: u16,
    pub(super) reason: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: Vec<u8>,
}

pub(super) fn first_mock(
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<Option<MockResponse>> {
    for item in actions {
        match &item.action {
            Action::Mock(value) => {
                let Ok((body, content_type)) = resolve_mock_value(value, item, meta, state) else {
                    continue;
                };
                return Ok(Some(MockResponse {
                    status: 200,
                    reason: "OK".to_string(),
                    headers: vec![("Content-Type".to_string(), content_type)],
                    body,
                }));
            }
            Action::MockRaw(value) => {
                let bytes = resolve_value_bytes(value, item, meta, state)?;
                return parse_raw_mock_response(&bytes).map(Some);
            }
            Action::MockInline(op) => {
                let status = op.status.unwrap_or(200);
                let body = match &op.body {
                    Some(value) => resolve_value_bytes(value, item, meta, state)?,
                    None => Vec::new(),
                };
                let mut headers = Vec::new();
                for (name, value) in &op.headers {
                    headers.push((name.clone(), resolve_value_text(value, item, meta, state)?));
                }
                if !headers
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case("content-type"))
                {
                    headers.push((
                        "Content-Type".to_string(),
                        "text/plain; charset=utf-8".to_string(),
                    ));
                }
                return Ok(Some(MockResponse {
                    status,
                    reason: http::reason_phrase(status).to_string(),
                    headers,
                    body,
                }));
            }
            _ => {}
        }
    }
    Ok(None)
}

pub(super) fn resolve_mock_value(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<(Vec<u8>, String)> {
    match value {
        Value::Inline(_) | Value::Reference(_) => Ok((
            resolve_value_bytes(value, item, meta, state)?,
            "text/plain; charset=utf-8".to_string(),
        )),
        Value::File(path) => {
            let rendered = item.render(path, meta);
            let mut last_err = None;
            for candidate in rendered
                .split('|')
                .map(str::trim)
                .filter(|candidate| !candidate.is_empty())
            {
                match read_rule_file_candidate(candidate, meta, state) {
                    Ok((body, resolved_path)) => {
                        return Ok((
                            render_text_bytes(body, item, meta),
                            content_type_for_path(&resolved_path),
                        ));
                    }
                    Err(err) => last_err = Some(err),
                }
            }
            Err(last_err.unwrap_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "mock file has no candidates")
            }))
        }
    }
}

pub(super) fn read_rule_file_candidate(
    path: &str,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<(Vec<u8>, String)> {
    let storage_path = state.config.storage.join(path);
    read_rule_file_path(&storage_path, meta)
        .or_else(|_| read_rule_file_path(Path::new(path), meta))
        .map(|(body, resolved)| {
            let display = resolved.to_string_lossy().into_owned();
            (body, display)
        })
}

pub(super) fn read_rule_file_path(
    path: &Path,
    meta: &RequestMeta,
) -> io::Result<(Vec<u8>, PathBuf)> {
    if path.is_dir() {
        let resolved = path.join(mock_directory_relative_path(meta));
        return fs::read(&resolved).map(|body| (body, resolved));
    }
    fs::read(path).map(|body| (body, path.to_path_buf()))
}

pub(super) fn mock_directory_relative_path(meta: &RequestMeta) -> PathBuf {
    let path = UrlParts::parse(&meta.url)
        .map(|url| url.path)
        .unwrap_or_else(|_| "/".to_string());
    let mut out = PathBuf::new();
    for segment in path.split('/') {
        let segment = segment.trim();
        if segment.is_empty() || segment == "." || segment == ".." {
            continue;
        }
        out.push(segment);
    }
    if out.as_os_str().is_empty() || path.ends_with('/') {
        out.push("index.html");
    }
    out
}

pub(super) fn content_type_for_path(path: &str) -> String {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let ext = Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "txt" | "text" | "log" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "js" | "mjs" => "application/javascript",
        "css" => "text/css",
        "xml" => "application/xml",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
    .to_string()
}

pub(super) fn parse_raw_mock_response(bytes: &[u8]) -> io::Result<MockResponse> {
    let (head, body) = if let Some(idx) = find_bytes(bytes, b"\r\n\r\n") {
        (&bytes[..idx], &bytes[idx + 4..])
    } else if let Some(idx) = find_bytes(bytes, b"\n\n") {
        (&bytes[..idx], &bytes[idx + 2..])
    } else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mock.raw must include status line, headers, blank line, and body",
        ));
    };
    let head = String::from_utf8_lossy(head);
    let mut lines = head.lines();
    let status_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "mock.raw missing status line"))?
        .trim_end_matches('\r');
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mock.raw status line must start with HTTP/",
        ));
    }
    let status = parts
        .next()
        .and_then(|value| value.parse::<u16>().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "mock.raw invalid status"))?;
    let reason = parts
        .next()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| http::reason_phrase(status))
        .to_string();
    let mut headers = Vec::new();
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("mock.raw invalid header `{line}`"),
            ));
        };
        let name = name.trim();
        if name.is_empty() || name.eq_ignore_ascii_case("connection") {
            continue;
        }
        headers.push((name.to_string(), value.trim_start().to_string()));
    }
    Ok(MockResponse {
        status,
        reason,
        headers,
        body: body.to_vec(),
    })
}

pub(super) fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
