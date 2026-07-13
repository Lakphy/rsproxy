use std::io::{self, Read, Write};

const RESPONSE_COALESCE_LIMIT: usize = 16 * 1024;
type Headers = Vec<(String, String)>;
type BodyAndTrailers = (Vec<u8>, Headers);

#[derive(Clone, Debug)]
pub(super) struct RawRequest {
    pub method: String,
    pub target: String,
    pub headers: Headers,
    pub body: Vec<u8>,
}

pub(super) fn read_request<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
    max_body_size: usize,
) -> io::Result<Option<RawRequest>> {
    let Some(head) = read_head(stream, max_header_size)? else {
        return Ok(None);
    };
    let text = std::str::from_utf8(&head)
        .map_err(|_| invalid_data("control request head is not valid UTF-8"))?;
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| invalid_data("missing control request line"))?;
    let mut request_parts = request_line.split_ascii_whitespace();
    let method = request_parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_data("missing control request method"))?;
    let target = request_parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_data("missing control request target"))?;
    let _version = request_parts
        .next()
        .filter(|value| matches!(*value, "HTTP/1.0" | "HTTP/1.1"))
        .ok_or_else(|| invalid_data("unsupported control HTTP version"))?;
    if request_parts.next().is_some() {
        return Err(invalid_data("invalid control request line"));
    }

    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if headers.len() >= max_header_count {
            return Err(invalid_data("control request header count exceeds limit"));
        }
        headers.push(parse_header(line)?);
    }

    let content_length = content_length(&headers)?;
    let chunked = transfer_chunked(&headers)?;
    if content_length.is_some() && chunked {
        return Err(invalid_data(
            "control request cannot combine Content-Length and Transfer-Encoding",
        ));
    }
    let (body, _trailers) = if chunked {
        read_chunked(stream, max_header_size, max_header_count, max_body_size)?
    } else if let Some(length) = content_length {
        if length > max_body_size {
            return Err(invalid_data(
                "control request body exceeds configured limit",
            ));
        }
        (read_exact_body(stream, length)?, Vec::new())
    } else {
        (Vec::new(), Vec::new())
    };

    Ok(Some(RawRequest {
        method: method.to_string(),
        target: target.to_string(),
        headers,
        body,
    }))
}

fn read_head<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut head = Vec::with_capacity(1024.min(max_header_size));
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) if head.is_empty() => return Ok(None),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated control request head",
                ));
            }
            Ok(_) => head.push(byte[0]),
            Err(error) => return Err(error),
        }
        if head.len() > max_header_size {
            return Err(invalid_data(
                "control request head exceeds configured limit",
            ));
        }
        if head.ends_with(b"\r\n\r\n") {
            head.truncate(head.len() - 4);
            return Ok(Some(head));
        }
    }
}

fn parse_header(line: &str) -> io::Result<(String, String)> {
    let (name, value) = line
        .split_once(':')
        .ok_or_else(|| invalid_data("malformed control request header"))?;
    if name.is_empty() || !name.bytes().all(header_name_byte) {
        return Err(invalid_data("invalid control request header name"));
    }
    Ok((name.to_string(), value.trim().to_string()))
}

fn header_name_byte(byte: u8) -> bool {
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

fn content_length(headers: &[(String, String)]) -> io::Result<Option<usize>> {
    let mut length = None;
    for (_, value) in headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
    {
        let parsed = value
            .parse::<usize>()
            .map_err(|_| invalid_data("invalid control request Content-Length"))?;
        if length.is_some_and(|existing| existing != parsed) {
            return Err(invalid_data(
                "conflicting control request Content-Length values",
            ));
        }
        length = Some(parsed);
    }
    Ok(length)
}

fn transfer_chunked(headers: &[(String, String)]) -> io::Result<bool> {
    let values = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if values.is_empty() {
        return Ok(false);
    }
    if values
        .last()
        .is_some_and(|value| value.eq_ignore_ascii_case("chunked"))
        && values[..values.len() - 1]
            .iter()
            .all(|value| value.eq_ignore_ascii_case("identity"))
    {
        return Ok(true);
    }
    Err(invalid_data(
        "unsupported control request Transfer-Encoding",
    ))
}

fn read_exact_body<R: Read + ?Sized>(stream: &mut R, length: usize) -> io::Result<Vec<u8>> {
    let mut body = Vec::with_capacity(length.min(8 * 1024));
    read_exact_append(stream, &mut body, length)?;
    Ok(body)
}

fn read_exact_append<R: Read + ?Sized>(
    stream: &mut R,
    body: &mut Vec<u8>,
    mut remaining: usize,
) -> io::Result<()> {
    let mut buffer = [0u8; 8 * 1024];
    while remaining != 0 {
        let wanted = remaining.min(buffer.len());
        let read = stream.read(&mut buffer[..wanted])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "truncated control request body",
            ));
        }
        body.try_reserve(read)
            .map_err(|_| invalid_data("control request body is too large"))?;
        body.extend_from_slice(&buffer[..read]);
        remaining -= read;
    }
    Ok(())
}

fn read_chunked<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
    max_body_size: usize,
) -> io::Result<BodyAndTrailers> {
    let mut body = Vec::new();
    loop {
        let line = read_crlf_line(stream, max_header_size)?;
        let size = line
            .split(';')
            .next()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .and_then(|value| usize::from_str_radix(value, 16).ok())
            .ok_or_else(|| invalid_data("invalid control request chunk size"))?;
        if size == 0 {
            break;
        }
        let end = body
            .len()
            .checked_add(size)
            .ok_or_else(|| invalid_data("control request body is too large"))?;
        if end > max_body_size {
            return Err(invalid_data(
                "control request body exceeds configured limit",
            ));
        }
        read_exact_append(stream, &mut body, size)?;
        let mut crlf = [0u8; 2];
        stream.read_exact(&mut crlf)?;
        if crlf != *b"\r\n" {
            return Err(invalid_data("invalid control request chunk terminator"));
        }
    }

    let mut trailers = Vec::new();
    let mut trailer_bytes = 0usize;
    loop {
        let line = read_crlf_line(stream, max_header_size)?;
        trailer_bytes = trailer_bytes.saturating_add(line.len() + 2);
        if trailer_bytes > max_header_size {
            return Err(invalid_data(
                "control request trailers exceed configured limit",
            ));
        }
        if line.is_empty() {
            break;
        }
        if trailers.len() >= max_header_count {
            return Err(invalid_data("control request trailer count exceeds limit"));
        }
        trailers.push(parse_header(&line)?);
    }
    Ok((body, trailers))
}

fn read_crlf_line<R: Read + ?Sized>(stream: &mut R, limit: usize) -> io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        line.push(byte[0]);
        if line.len() > limit {
            return Err(invalid_data("control request framing line exceeds limit"));
        }
        if line.ends_with(b"\r\n") {
            line.truncate(line.len() - 2);
            return String::from_utf8(line)
                .map_err(|_| invalid_data("control request framing is not valid UTF-8"));
        }
    }
}

pub(super) fn write_response<W: Write + ?Sized>(
    stream: &mut W,
    status: u16,
    reason: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> io::Result<()> {
    let mut encoded = Vec::with_capacity(256 + body.len().min(RESPONSE_COALESCE_LIMIT));
    write!(encoded, "HTTP/1.1 {status} {reason}\r\n")?;
    let mut has_length = false;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("connection") {
            continue;
        }
        has_length |= name.eq_ignore_ascii_case("content-length");
        write!(encoded, "{name}: {value}\r\n")?;
    }
    if !has_length {
        write!(encoded, "Content-Length: {}\r\n", body.len())?;
    }
    write!(encoded, "Connection: close\r\n\r\n")?;
    if body.len() <= RESPONSE_COALESCE_LIMIT {
        encoded.extend_from_slice(body);
        stream.write_all(&encoded)?;
    } else {
        stream.write_all(&encoded)?;
        stream.write_all(body)?;
    }
    stream.flush()
}

pub(super) fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Content Too Large",
        431 => "Request Header Fields Too Large",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "OK",
    }
}

fn invalid_data(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
#[path = "http/tests.rs"]
mod tests;
