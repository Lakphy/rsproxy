use super::*;

/// Writes a complete HTTP/1.1 response and flushes the destination.
pub fn write_response<W: Write + ?Sized>(
    stream: &mut W,
    status: u16,
    reason: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> io::Result<()> {
    write_response_with_connection(stream, status, reason, headers, body, false)
}

pub(crate) fn write_response_with_connection<W: Write + ?Sized>(
    stream: &mut W,
    status: u16,
    reason: &str,
    headers: &[(String, String)],
    body: &[u8],
    keep_alive: bool,
) -> io::Result<()> {
    write_response_with_version_and_connection(
        stream, "HTTP/1.1", status, reason, headers, body, keep_alive,
    )
}

/// Writes a complete response for an explicit HTTP version and connection policy.
pub fn write_response_with_version_and_connection<W: Write + ?Sized>(
    stream: &mut W,
    version: &str,
    status: u16,
    reason: &str,
    headers: &[(String, String)],
    body: &[u8],
    keep_alive: bool,
) -> io::Result<()> {
    validate_response_parts(version, status, reason, headers)?;
    if !status_can_send_content(status) && !body.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("HTTP status {status} does not allow response content"),
        ));
    }
    let mut head = Vec::with_capacity(256);
    write!(&mut head, "{version} {status} {reason}\r\n")?;
    for (name, value) in headers {
        if is_complete_response_managed_header(name) {
            continue;
        }
        write!(&mut head, "{name}: {value}\r\n")?;
    }
    if status_can_send_content(status) {
        write!(&mut head, "Content-Length: {}\r\n", body.len())?;
    } else if status == 205 {
        write!(&mut head, "Content-Length: 0\r\n")?;
    }
    write!(
        &mut head,
        "Connection: {}\r\n\r\n",
        if keep_alive { "keep-alive" } else { "close" }
    )?;
    if body.len() <= COALESCED_RESPONSE_LIMIT {
        head.extend_from_slice(body);
        stream.write_all(&head)?;
    } else {
        stream.write_all(&head)?;
        stream.write_all(body)?;
    }
    stream.flush()
}

/// Writes a response status line and headers without body bytes.
pub fn write_response_head<W: Write + ?Sized>(
    stream: &mut W,
    head: &RawResponseHead,
    headers: &[(String, String)],
) -> io::Result<()> {
    write_response_head_with_connection(stream, head, headers, false)
}

/// Writes a response head after normalizing its connection header.
pub fn write_response_head_with_connection<W: Write + ?Sized>(
    stream: &mut W,
    head: &RawResponseHead,
    headers: &[(String, String)],
    keep_alive: bool,
) -> io::Result<()> {
    let reason = if head.reason.is_empty() {
        reason_phrase(head.status)
    } else {
        &head.reason
    };
    validate_response_parts(&head.version, head.status, reason, headers)?;
    let content_length = validate_streaming_framing(headers)?;
    let capacity = headers
        .iter()
        .map(|(name, value)| name.len() + value.len() + 4)
        .sum::<usize>()
        .saturating_add(96);
    let mut encoded = Vec::with_capacity(capacity);
    write!(encoded, "{} {} {}\r\n", head.version, head.status, reason)?;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("connection")
            || name.eq_ignore_ascii_case("content-length")
            || (!status_can_send_content(head.status) && matches_body_framing_header(name))
        {
            continue;
        }
        write!(encoded, "{name}: {value}\r\n")?;
    }
    if status_can_send_content(head.status) {
        if let Some(content_length) = content_length {
            write!(encoded, "Content-Length: {content_length}\r\n")?;
        }
    } else if head.status == 205 {
        write!(encoded, "Content-Length: 0\r\n")?;
    }
    write!(
        encoded,
        "Connection: {}\r\n\r\n",
        if keep_alive { "keep-alive" } else { "close" }
    )?;
    stream.write_all(&encoded)
}

/// Reports whether a response status permits a sender to generate content.
pub const fn status_can_send_content(status: u16) -> bool {
    rsproxy_http::status_can_send_content(status)
}

/// Reports whether a response may send content for the request method and status.
pub fn response_can_send_content(method: &str, status: u16) -> bool {
    rsproxy_http::response_can_send_content(method, status)
}

/// Reports whether an upstream response is framed as carrying body bytes.
pub fn response_has_framed_body(method: &str, status: u16) -> bool {
    rsproxy_http::response_has_framed_body(method, status)
}

fn validate_response_parts(
    version: &str,
    status: u16,
    reason: &str,
    headers: &[(String, String)],
) -> io::Result<()> {
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid HTTP response version `{version}`"),
        ));
    }
    if !(100..=599).contains(&status) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid HTTP response status `{status}`"),
        ));
    }
    if !reason.bytes().all(is_http_field_value_byte) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid HTTP response reason phrase",
        ));
    }
    for (name, value) in headers {
        if name.is_empty() || !name.bytes().all(is_http_token_byte) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid HTTP response header name `{name}`"),
            ));
        }
        if !value.bytes().all(is_http_field_value_byte) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid value for HTTP response header `{name}`"),
            ));
        }
    }
    Ok(())
}

fn validate_streaming_framing(headers: &[(String, String)]) -> io::Result<Option<u64>> {
    let mut content_length = None;
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
    {
        for value in value.split(',').map(str::trim) {
            if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid HTTP response Content-Length header",
                ));
            }
            let parsed = value.parse::<u64>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid HTTP response Content-Length header",
                )
            })?;
            if content_length.is_some_and(|existing| existing != parsed) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "conflicting HTTP response Content-Length headers",
                ));
            }
            content_length = Some(parsed);
        }
    }
    if content_length.is_some()
        && headers
            .iter()
            .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "HTTP response cannot contain both Content-Length and Transfer-Encoding",
        ));
    }
    Ok(content_length)
}

fn matches_body_framing_header(name: &str) -> bool {
    ["content-length", "trailer", "transfer-encoding"]
        .iter()
        .any(|framing| name.eq_ignore_ascii_case(framing))
}

fn is_complete_response_managed_header(name: &str) -> bool {
    [
        "connection",
        "content-length",
        "keep-alive",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
    ]
    .iter()
    .any(|managed| name.eq_ignore_ascii_case(managed))
}

fn is_http_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn is_http_field_value_byte(byte: u8) -> bool {
    byte == b'\t' || (byte >= b' ' && byte != 0x7f)
}

/// Returns the first ASCII-case-insensitive header value.
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

/// Replaces the first matching header or appends one canonical entry.
pub fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: String) {
    if let Some((_, existing)) = headers
        .iter_mut()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
    {
        *existing = value;
    } else {
        headers.push((canonical_header_name(name), value));
    }
}

/// Removes all ASCII-case-insensitive occurrences of a header.
pub fn remove_header(headers: &mut Vec<(String, String)>, name: &str) {
    headers.retain(|(header_name, _)| !header_name.eq_ignore_ascii_case(name));
}

/// Returns the standard reason phrase used by the HTTP/1 response writer.
pub fn reason_phrase(status: u16) -> &'static str {
    ::http::StatusCode::from_u16(status)
        .ok()
        .and_then(|status| status.canonical_reason())
        .unwrap_or("")
}
