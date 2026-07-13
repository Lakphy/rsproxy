use super::*;

pub(in crate::proxy) fn is_sse_response(headers: &[(String, String)]) -> bool {
    http::header(headers, "content-type")
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("text/event-stream")
        })
        .unwrap_or(false)
}

pub(in crate::proxy) fn accepts_sse(headers: &[(String, String)]) -> bool {
    http::header(headers, "accept").is_some_and(|value| {
        value.split(',').any(|item| {
            item.split(';')
                .next()
                .is_some_and(|media| media.trim().eq_ignore_ascii_case("text/event-stream"))
        })
    })
}

pub(in crate::proxy) fn sse_frames(body: &[u8]) -> Vec<FrameRecord> {
    let text = String::from_utf8_lossy(body).replace("\r\n", "\n");
    text.split("\n\n")
        .filter(|frame| !frame.trim().is_empty())
        .take(512)
        .map(|frame| FrameRecord {
            direction: FrameDirection::ServerToClient,
            at_ms: rsproxy_trace::now_millis(),
            opcode: "sse".to_string(),
            fin: true,
            payload_len: frame.len() as u64,
            data_encoding: FrameDataEncoding::Utf8,
            data: frame.as_bytes().to_vec(),
            truncated: false,
        })
        .collect()
}

pub(in crate::proxy) fn parse_query_pairs(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            (k.to_string(), v.to_string())
        })
        .collect()
}

pub(in crate::proxy) fn url_to_string(url: &UrlParts) -> String {
    let authority = match url.port {
        Some(port) => format_host_port(&url.host, port),
        None => format_authority_host(&url.host),
    };
    format!("{}://{}{}", url.scheme, authority, url.origin_form())
}

pub(in crate::proxy) fn first_status(actions: &[ResolvedAction]) -> Option<(u16, &ResolvedAction)> {
    actions.iter().find_map(|item| match item.action {
        Action::Status(code) => Some((code, item)),
        _ => None,
    })
}

pub(in crate::proxy) fn first_redirect(
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<Option<(String, u16)>> {
    for item in actions {
        if let Action::Redirect { url, code } = &item.action {
            return Ok(Some((resolve_value_text(url, item, meta, state)?, *code)));
        }
    }
    Ok(None)
}
