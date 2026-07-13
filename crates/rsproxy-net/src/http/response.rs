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
    let mut head = Vec::with_capacity(256);
    write!(&mut head, "{version} {status} {reason}\r\n")?;
    let mut has_len = false;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("connection") {
            continue;
        }
        if name.eq_ignore_ascii_case("content-length") {
            has_len = true;
        }
        write!(&mut head, "{name}: {value}\r\n")?;
    }
    if !has_len {
        write!(&mut head, "Content-Length: {}\r\n", body.len())?;
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
    let capacity = headers
        .iter()
        .map(|(name, value)| name.len() + value.len() + 4)
        .sum::<usize>()
        .saturating_add(96);
    let mut encoded = Vec::with_capacity(capacity);
    write!(encoded, "{} {} {}\r\n", head.version, head.status, reason)?;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("connection") {
            continue;
        }
        write!(encoded, "{name}: {value}\r\n")?;
    }
    write!(
        encoded,
        "Connection: {}\r\n\r\n",
        if keep_alive { "keep-alive" } else { "close" }
    )?;
    stream.write_all(&encoded)
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
    match status {
        200 => "OK",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        307 => "Temporary Redirect",
        400 => "Bad Request",
        407 => "Proxy Authentication Required",
        410 => "Gone",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "OK",
    }
}
