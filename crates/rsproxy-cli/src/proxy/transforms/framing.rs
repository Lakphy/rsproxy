use super::*;

pub(in crate::proxy) fn strip_hop_by_hop_headers(headers: &mut Vec<(String, String)>) {
    let connection_tokens = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("connection"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    for name in connection_tokens {
        http::remove_header(headers, &name);
    }
    for name in [
        "connection",
        "keep-alive",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ] {
        http::remove_header(headers, name);
    }
}

pub(in crate::proxy) fn prepare_streaming_body_headers(headers: &mut Vec<(String, String)>) {
    http::remove_header(headers, "content-length");
    http::remove_header(headers, "transfer-encoding");
    http::remove_header(headers, "trailer");
}

pub(in crate::proxy) fn can_stream_sse_response(actions: &[ResolvedAction]) -> bool {
    !actions.iter().any(|item| {
        matches!(
            item.action,
            Action::ResBody(_) | Action::ResMerge(_) | Action::ResTrailer(_) | Action::Inject(_)
        )
    })
}

pub(in crate::proxy) fn prepare_trailer_headers(
    head: &mut http::RawResponseHead,
    headers: &mut Vec<(String, String)>,
    trailers: &[(String, String)],
) {
    head.version = "HTTP/1.1".to_string();
    http::remove_header(headers, "content-length");
    http::remove_header(headers, "transfer-encoding");
    http::set_header(headers, "Transfer-Encoding", "chunked".to_string());
    http::set_header(
        headers,
        "Trailer",
        trailers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
}

pub(in crate::proxy) fn prepare_upstream_request_framing(
    headers: &mut Vec<(String, String)>,
    request: &RawRequest,
) {
    let had_content_length = http::header(headers, "content-length").is_some();
    http::remove_header(headers, "transfer-encoding");
    http::remove_header(headers, "trailer");
    if request.trailers.is_empty() {
        if !request.body.is_empty() || had_content_length {
            http::set_header(headers, "Content-Length", request.body.len().to_string());
        } else {
            http::remove_header(headers, "content-length");
        }
        return;
    }
    http::remove_header(headers, "content-length");
    http::set_header(headers, "Transfer-Encoding", "chunked".to_string());
    http::set_header(
        headers,
        "Trailer",
        request
            .trailers
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
}

pub(in crate::proxy) fn prepare_streaming_upstream_request_framing(
    headers: &mut Vec<(String, String)>,
    framing: http::RequestBodyFraming,
) {
    http::remove_header(headers, "content-length");
    http::remove_header(headers, "transfer-encoding");
    match framing {
        http::RequestBodyFraming::None => {
            http::remove_header(headers, "trailer");
        }
        http::RequestBodyFraming::ContentLength(length) => {
            http::remove_header(headers, "trailer");
            http::set_header(headers, "Content-Length", length.to_string());
        }
        http::RequestBodyFraming::Chunked => {
            http::set_header(headers, "Transfer-Encoding", "chunked".to_string());
        }
    }
}

#[cfg(test)]
pub(in crate::proxy) fn write_chunked_request<W: Write + ?Sized>(
    stream: &mut W,
    body: &[u8],
    trailers: &[(String, String)],
    bytes_per_sec: Option<u64>,
) -> io::Result<()> {
    write_chunked_request_inner(stream, body, trailers, bytes_per_sec, None)
}

pub(in crate::proxy) fn write_chunked_request_until<W: Write + ?Sized>(
    stream: &mut W,
    body: &[u8],
    trailers: &[(String, String)],
    bytes_per_sec: Option<u64>,
    deadline: RequestDeadline,
) -> io::Result<()> {
    write_chunked_request_inner(stream, body, trailers, bytes_per_sec, Some(deadline))
}

fn write_chunked_request_inner<W: Write + ?Sized>(
    stream: &mut W,
    body: &[u8],
    trailers: &[(String, String)],
    bytes_per_sec: Option<u64>,
    deadline: Option<RequestDeadline>,
) -> io::Result<()> {
    if !body.is_empty() {
        write!(stream, "{:X}\r\n", body.len())?;
        match deadline {
            Some(deadline) => write_maybe_throttled_until(stream, body, bytes_per_sec, deadline)?,
            None => write_maybe_throttled(stream, body, bytes_per_sec)?,
        }
        write!(stream, "\r\n")?;
    }
    write!(stream, "0\r\n")?;
    for (name, value) in trailers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")
}

pub(in crate::proxy) fn write_chunked_response<W: Write + ?Sized>(
    stream: &mut W,
    head: &http::RawResponseHead,
    headers: &[(String, String)],
    body: &[u8],
    trailers: &[(String, String)],
    bytes_per_sec: Option<u64>,
    client_connection: ClientPersistence,
) -> io::Result<()> {
    http::write_response_head_with_connection(
        stream,
        head,
        headers,
        client_connection.keep_alive(),
    )?;
    if !body.is_empty() {
        write!(stream, "{:X}\r\n", body.len())?;
        write_maybe_throttled(stream, body, bytes_per_sec)?;
        write!(stream, "\r\n")?;
    }
    write!(stream, "0\r\n")?;
    for (name, value) in trailers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")
}
