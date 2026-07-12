use super::*;

mod body;
mod head;

pub(crate) use body::{
    BoundedRequestBody, RequestBodyRead, RequestBodyReader, read_request_body_all,
    read_request_body_bounded, validate_request_trailers,
};

#[derive(Clone, Debug)]
pub(crate) struct RawRequest {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub trailers: Vec<(String, String)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RequestBodyFraming {
    None,
    ContentLength(usize),
    Chunked,
}

impl RequestBodyFraming {
    pub(crate) fn has_body(self) -> bool {
        !matches!(self, Self::None | Self::ContentLength(0))
    }
}

#[derive(Debug)]
pub(crate) struct RequestHead {
    pub(crate) request: RawRequest,
    pub(crate) body: RequestBodyFraming,
}

pub(crate) fn read_request_head<R: Read + ?Sized>(
    stream: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Option<RequestHead>> {
    head::read(stream, max_header_size, max_header_count)
}

pub(crate) fn read_request_head_tcp(
    stream: &mut std::net::TcpStream,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Option<RequestHead>> {
    head::read_tcp(stream, max_header_size, max_header_count)
}

pub(crate) fn read_request<R: Read + ?Sized>(
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
