use super::*;
use cap_std::{ambient_authority, fs::Dir};
use std::path::Component;

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
                let (body, content_type) = resolve_mock_value(value, item, meta, state)?;
                let response = MockResponse {
                    status: 200,
                    reason: "OK".to_string(),
                    headers: vec![("Content-Type".to_string(), content_type)],
                    body,
                };
                return finalize_mock_response(response, state).map(Some);
            }
            Action::MockRaw(value) => {
                let bytes =
                    resolve_value_bytes_bounded(value, item, meta, state, rule_body_limit(state))?;
                let response = parse_raw_mock_response(&bytes)?;
                return finalize_mock_response(response, state).map(Some);
            }
            Action::MockInline(op) => {
                let status = op.status.unwrap_or(200);
                let body = match &op.body {
                    Some(value) => resolve_value_bytes_bounded(
                        value,
                        item,
                        meta,
                        state,
                        rule_body_limit(state),
                    )?,
                    None => Vec::new(),
                };
                let mut headers = Vec::new();
                for (name, value) in &op.headers {
                    headers.push((
                        name.clone(),
                        resolve_value_text_bounded(
                            value,
                            item,
                            meta,
                            state,
                            rule_header_limit(state),
                        )?,
                    ));
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
                return finalize_mock_response(
                    MockResponse {
                        status,
                        reason: http::reason_phrase(status).to_string(),
                        headers,
                        body,
                    },
                    state,
                )
                .map(Some);
            }
            _ => {}
        }
    }
    Ok(None)
}

pub(super) fn finalize_mock_response(
    mut response: MockResponse,
    state: &SharedState,
) -> io::Result<MockResponse> {
    if !(rsproxy_rules::MIN_FINAL_HTTP_STATUS..=rsproxy_rules::MAX_HTTP_STATUS)
        .contains(&response.status)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "mock response status must be in {}..={}",
                rsproxy_rules::MIN_FINAL_HTTP_STATUS,
                rsproxy_rules::MAX_HTTP_STATUS
            ),
        ));
    }
    if !http::status_can_send_content(response.status) && !response.body.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "mock response status {} must not include content",
                response.status
            ),
        ));
    }
    strip_hop_by_hop_headers(&mut response.headers);
    http::remove_header(&mut response.headers, "content-length");
    validate_header_block(&response.headers, state)?;
    Ok(response)
}

pub(super) fn resolve_mock_value(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<(Vec<u8>, String)> {
    match value {
        Value::Inline(_) | Value::Reference(_) => Ok((
            resolve_value_bytes_bounded(value, item, meta, state, rule_body_limit(state))?,
            "text/plain; charset=utf-8".to_string(),
        )),
        Value::File(path) => {
            let rendered = render_rule_path(path, item, meta)?;
            let mut last_err = None;
            let candidates = rendered
                .split('|')
                .map(str::trim)
                .filter(|candidate| !candidate.is_empty())
                .take(rsproxy_rules::MAX_RULE_MOCK_FILE_CANDIDATES + 1)
                .collect::<Vec<_>>();
            if candidates.len() > rsproxy_rules::MAX_RULE_MOCK_FILE_CANDIDATES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "mock file exceeds the {}-candidate limit",
                        rsproxy_rules::MAX_RULE_MOCK_FILE_CANDIDATES
                    ),
                ));
            }
            for candidate in candidates {
                match read_rule_file_candidate(candidate, meta, state) {
                    Ok((body, resolved_path)) => {
                        return Ok((
                            render_text_bytes(body, item, meta, rule_body_limit(state))?,
                            content_type_for_path(&resolved_path),
                        ));
                    }
                    Err(err) if err.kind() == io::ErrorKind::NotFound => last_err = Some(err),
                    Err(err) => return Err(err),
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
    read_rule_file_path(&storage_path, meta, rule_body_limit(state))
        .or_else(|error| {
            if error.kind() == io::ErrorKind::NotFound {
                read_rule_file_path(Path::new(path), meta, rule_body_limit(state))
            } else {
                Err(error)
            }
        })
        .map(|(body, resolved)| {
            let display = resolved.to_string_lossy().into_owned();
            (body, display)
        })
}

pub(super) fn read_rule_file_path(
    path: &Path,
    meta: &RequestMeta,
    limit: usize,
) -> io::Result<(Vec<u8>, PathBuf)> {
    if path.is_dir() {
        let base = fs::canonicalize(path)?;
        let relative = mock_directory_relative_path(meta)?;
        let resolved = base.join(&relative);
        let directory = Dir::open_ambient_dir(path, ambient_authority())?;
        let file = directory.open(&relative)?;
        return crate::bounded_io::read_open_file(
            file,
            &resolved,
            limit.min(rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES),
            "mock file",
        )
        .map(|body| (body, resolved));
    }
    crate::bounded_io::read_file(
        path,
        limit.min(rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES),
        "mock file",
    )
    .map(|body| (body, path.to_path_buf()))
}

pub(super) fn mock_directory_relative_path(meta: &RequestMeta) -> io::Result<PathBuf> {
    let path = UrlParts::parse(&meta.url)
        .map(|url| url.path)
        .unwrap_or_else(|_| "/".to_string());
    let mut out = PathBuf::new();
    for segment in path.split('/') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        if matches!(segment, "." | "..")
            || segment.contains(['\\', ':', '\0'])
            || !is_safe_path_segment(segment)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unsafe mock directory URL path segment `{segment}`"),
            ));
        }
        out.push(segment);
    }
    if out.as_os_str().is_empty() || path.ends_with('/') {
        out.push("index.html");
    }
    Ok(out)
}

fn is_safe_path_segment(segment: &str) -> bool {
    let mut components = Path::new(segment).components();
    matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
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
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "mock.raw status line must use HTTP/1.0 or HTTP/1.1",
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
