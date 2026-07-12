use std::io::{self, BufRead, Read, Write};

mod request;
mod response;

#[cfg(test)]
pub(crate) use request::read_request_body_all;
pub(crate) use request::{
    BoundedRequestBody, RawRequest, RequestBodyFraming, RequestBodyRead, RequestBodyReader,
    RequestHead, read_request, read_request_body_bounded, read_request_head, read_request_head_tcp,
    validate_request_trailers,
};
#[cfg(test)]
pub(crate) use response::write_response_with_connection;
pub(crate) use response::{
    header, reason_phrase, remove_header, set_header, write_response, write_response_head,
    write_response_head_with_connection, write_response_with_version_and_connection,
};

const COALESCED_RESPONSE_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct RawResponseHead {
    pub version: String,
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
}

pub fn read_response_head<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<RawResponseHead> {
    let head = read_head(stream, max_header_size)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "empty response"))?;
    parse_response_head(&head, max_header_count)
}

pub fn read_response_head_buffered<R: BufRead + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<RawResponseHead> {
    let head = read_head_buffered(stream, max_header_size)?
        .ok_or_else(|| io::Error::new(io::ErrorKind::UnexpectedEof, "empty response"))?;
    parse_response_head(&head, max_header_count)
}

fn parse_response_head(head: &[u8], max_header_count: usize) -> io::Result<RawResponseHead> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty response"))?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();
    let status = parts.next().unwrap_or("502").parse::<u16>().unwrap_or(502);
    let reason = parts.next().unwrap_or("").to_string();
    Ok(RawResponseHead {
        version,
        status,
        reason,
        headers: parse_headers_limited(lines, max_header_count)?,
    })
}

fn read_head_buffered<R: BufRead + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
) -> io::Result<Option<Vec<u8>>> {
    const TERMINATOR: &[u8; 4] = b"\r\n\r\n";
    const PREFIX: [usize; 4] = [0, 0, 1, 2];

    let mut buf = Vec::with_capacity(1024);
    let mut matched = 0usize;
    loop {
        let (consumed, complete) = {
            let available = stream.fill_buf()?;
            if available.is_empty() {
                if buf.is_empty() {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before headers completed",
                ));
            }

            let mut consumed = available.len();
            let mut complete = false;
            for (index, byte) in available.iter().copied().enumerate() {
                while matched > 0 && byte != TERMINATOR[matched] {
                    matched = PREFIX[matched - 1];
                }
                if byte == TERMINATOR[matched] {
                    matched += 1;
                }
                if matched == TERMINATOR.len() {
                    consumed = index + 1;
                    complete = true;
                    break;
                }
            }
            buf.extend_from_slice(&available[..consumed]);
            (consumed, complete)
        };
        stream.consume(consumed);

        if buf.len() > max_header_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "header size limit exceeded",
            ));
        }
        if complete {
            buf.truncate(buf.len() - TERMINATOR.len());
            return Ok(Some(buf));
        }
    }
}

fn read_head<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
) -> io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::with_capacity(1024);
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) if buf.is_empty() => return Ok(None),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed before headers completed",
                ));
            }
            Ok(_) => {
                buf.push(byte[0]);
                if buf.len() > max_header_size {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "header size limit exceeded",
                    ));
                }
                if buf.ends_with(b"\r\n\r\n") {
                    buf.truncate(buf.len() - 4);
                    return Ok(Some(buf));
                }
            }
            Err(err) => return Err(err),
        }
    }
}

fn parse_headers_limited<'a>(
    lines: impl Iterator<Item = &'a str>,
    max_header_count: usize,
) -> io::Result<Vec<(String, String)>> {
    let mut count = 0usize;
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        count += 1;
        if count > max_header_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("header count limit exceeded (limit {max_header_count})"),
            ));
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    Ok(headers)
}

fn canonical_header_name(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
#[path = "http/tests/mod.rs"]
mod tests;
