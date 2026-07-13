use super::*;

mod body;
mod head;

pub(crate) use body::read_request_body_all;
pub use body::{
    BoundedRequestBody, RequestBodyRead, RequestBodyReader, read_request_body_bounded,
    validate_request_trailers,
};

#[derive(Clone, Debug)]
/// Fully buffered HTTP/1 request in normalized string-and-byte form.
pub struct RawRequest {
    /// Request method token.
    pub method: String,
    /// Origin-form or absolute-form request target as received.
    pub target: String,
    /// Wire HTTP version token.
    pub version: String,
    /// Header fields in wire order.
    pub headers: Vec<(String, String)>,
    /// Decoded request body bytes.
    pub body: Vec<u8>,
    /// Decoded chunked trailer fields.
    pub trailers: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Framing selected from validated HTTP/1 request headers.
pub enum RequestBodyFraming {
    /// No request body follows the head.
    None,
    /// Exactly the contained number of bytes follows.
    ContentLength(usize),
    /// Transfer-Encoding chunk framing follows.
    Chunked,
}

impl RequestBodyFraming {
    /// Returns whether framing can produce at least one body byte.
    pub fn has_body(self) -> bool {
        !matches!(self, Self::None | Self::ContentLength(0))
    }
}

#[derive(Debug)]
/// Parsed HTTP/1 request head plus the body framing needed to continue reading.
pub struct RequestHead {
    /// Request metadata with empty body and trailer vectors.
    pub request: RawRequest,
    /// Validated framing for the unread body.
    pub body: RequestBodyFraming,
}

/// Reads one HTTP/1 request head without consuming its body.
pub fn read_request_head<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Option<RequestHead>> {
    head::read(stream, max_header_size, max_header_count)
}

/// Reads one request head from a TCP stream using readiness-aware peeking.
pub fn read_request_head_tcp(
    stream: &mut std::net::TcpStream,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Option<RequestHead>> {
    head::read_tcp(stream, max_header_size, max_header_count)
}

/// Reads one complete HTTP/1 request, including decoded body and trailers.
///
/// Header limits apply both to the initial head and to chunked trailers.
pub fn read_request<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Option<RawRequest>> {
    let Some(mut head) = read_request_head(stream, max_header_size, max_header_count)? else {
        return Ok(None);
    };
    let (body, trailers) = read_request_body_all(
        stream,
        RequestBodyReader::new(head.body),
        max_header_size,
        max_header_count,
    )?;
    head.request.body = body;
    head.request.trailers = trailers;
    Ok(Some(head.request))
}
