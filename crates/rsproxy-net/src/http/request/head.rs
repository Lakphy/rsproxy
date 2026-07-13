use std::io::{self, Read};
use std::net::TcpStream;

use super::{RawRequest, RequestBodyFraming, RequestHead};
use crate::http::{parse_headers_limited, read_head};

pub(super) fn read<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Option<RequestHead>> {
    let Some(head) = read_head(stream, max_header_size)? else {
        return Ok(None);
    };
    parse(&head, max_header_count).map(Some)
}

pub(super) fn read_tcp(
    stream: &mut TcpStream,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Option<RequestHead>> {
    const PEEK_CAPACITY: usize = 4 * 1024;

    let mut peeked = [0u8; PEEK_CAPACITY];
    let peek_limit = peeked.len().min(max_header_size.saturating_add(1)).max(1);
    let available = stream.peek(&mut peeked[..peek_limit])?;
    if available == 0 {
        return Ok(None);
    }
    if let Some(end) = peeked[..available]
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|position| position + 4)
    {
        if end > max_header_size {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "header size limit exceeded",
            ));
        }
        stream.read_exact(&mut peeked[..end])?;
        return parse(&peeked[..end - 4], max_header_count).map(Some);
    }
    read(stream, max_header_size, max_header_count)
}

fn parse(head: &[u8], max_header_count: usize) -> io::Result<RequestHead> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "empty request"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("").to_string();
    let version = parts.next().unwrap_or("HTTP/1.1").to_string();
    if method.is_empty() || target.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid request line",
        ));
    }

    let headers = parse_headers_limited(lines, max_header_count)?;
    let framing = body_framing(&headers)?;
    Ok(RequestHead {
        request: RawRequest {
            method,
            target,
            version,
            headers,
            body: Vec::new(),
            trailers: Vec::new(),
        },
        body: framing,
    })
}

fn body_framing(headers: &[(String, String)]) -> io::Result<RequestBodyFraming> {
    let content_length = content_length(headers)?;
    let transfer_codings = transfer_codings(headers);
    if !transfer_codings.is_empty() && content_length.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "request must not contain both Content-Length and Transfer-Encoding",
        ));
    }
    if transfer_codings.is_empty() {
        return Ok(content_length
            .map(RequestBodyFraming::ContentLength)
            .unwrap_or(RequestBodyFraming::None));
    }
    if transfer_codings.len() == 1 && transfer_codings[0] == "chunked" {
        return Ok(RequestBodyFraming::Chunked);
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "unsupported request Transfer-Encoding: {}",
            transfer_codings.join(", ")
        ),
    ))
}

fn content_length(headers: &[(String, String)]) -> io::Result<Option<usize>> {
    let values = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .collect::<Vec<_>>();
    let Some(first) = values.first() else {
        return Ok(None);
    };
    let parsed = first
        .parse::<usize>()
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid Content-Length header"))?;
    if values.iter().skip(1).any(|value| {
        value
            .parse::<usize>()
            .map(|value| value != parsed)
            .unwrap_or(true)
    }) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "conflicting Content-Length headers",
        ));
    }
    Ok(Some(parsed))
}

fn transfer_codings(headers: &[(String, String)]) -> Vec<String> {
    headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}
