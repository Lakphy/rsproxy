use std::io::{self, Read};

pub(super) fn read_trailers<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Vec<(String, String)>> {
    let mut trailers = Vec::new();
    let mut total_size = 0usize;
    loop {
        let line = read_crlf_line_limited(stream, max_header_size)?;
        total_size = total_size.saturating_add(line.len()).saturating_add(2);
        if total_size > max_header_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("trailer size limit exceeded (limit {max_header_size})"),
            ));
        }
        if line.is_empty() {
            return Ok(trailers);
        }
        if trailers.len() >= max_header_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("trailer count limit exceeded (limit {max_header_count})"),
            ));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid request trailer"))?;
        let name = name.trim();
        if name.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "empty request trailer name",
            ));
        }
        let value = value.trim();
        validate_request_trailer(name, value)?;
        trailers.push((name.to_string(), value.to_string()));
    }
}

/// Validates trailer count, aggregate size, names, and forbidden framing fields.
pub fn validate_request_trailers(
    trailers: &[(String, String)],
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<()> {
    if trailers.len() > max_header_count {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("trailer count limit exceeded (limit {max_header_count})"),
        ));
    }
    let mut size = 0usize;
    for (name, value) in trailers {
        validate_request_trailer(name, value)?;
        size = size
            .saturating_add(name.len())
            .saturating_add(value.len())
            .saturating_add(32);
    }
    if size > max_header_size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("trailer size limit exceeded (limit {max_header_size})"),
        ));
    }
    Ok(())
}

fn validate_request_trailer(name: &str, value: &str) -> io::Result<()> {
    if name.is_empty() || !name.bytes().all(is_header_token_byte) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid request trailer name `{name}`"),
        ));
    }
    if request_trailer_forbidden(name) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("forbidden request trailer `{name}`"),
        ));
    }
    if value.bytes().any(|byte| matches!(byte, b'\r' | b'\n' | 0)) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid value for request trailer `{name}`"),
        ));
    }
    Ok(())
}

fn request_trailer_forbidden(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "connection"
            | "content-length"
            | "cookie"
            | "host"
            | "keep-alive"
            | "proxy-authorization"
            | "proxy-connection"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn is_header_token_byte(byte: u8) -> bool {
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

pub(super) fn read_crlf_line_limited<R: Read + ?Sized>(
    stream: &mut R,
    limit: usize,
) -> io::Result<String> {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        bytes.push(byte[0]);
        if bytes.len() > limit.saturating_add(2) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("chunk line size limit exceeded (limit {limit})"),
            ));
        }
        if bytes.ends_with(b"\r\n") {
            bytes.truncate(bytes.len() - 2);
            return String::from_utf8(bytes).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "request chunk line is not UTF-8",
                )
            });
        }
    }
}
