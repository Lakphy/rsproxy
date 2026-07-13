use super::{BoundedRequestBody, RequestBodyRead, RequestBodyReader};
use crate::http::RequestBodyFraming;
use std::io::{self, Read};

const REQUEST_BODY_CHUNK_SIZE: usize = 16 * 1024;
type BodyAndTrailers = (Vec<u8>, Vec<(String, String)>);

/// Buffers a request body until it ends or its decoded byte limit is exceeded.
///
/// On overflow, the returned reader retains framing state so the caller can
/// stream the remainder without reparsing already consumed bytes.
pub fn read_request_body_bounded<R: Read + ?Sized>(
    stream: &mut R,
    framing: RequestBodyFraming,
    limit: usize,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<BoundedRequestBody> {
    let reader = RequestBodyReader::new(framing);
    if matches!(framing, RequestBodyFraming::ContentLength(length) if length > limit) {
        return Ok(BoundedRequestBody::Overflow {
            prefix: Vec::new(),
            reader,
        });
    }

    let mut reader = reader;
    let mut body = Vec::with_capacity(limit.min(64 * 1024));
    let mut buffer = [0u8; REQUEST_BODY_CHUNK_SIZE];
    loop {
        let allowed = limit.saturating_add(1).saturating_sub(body.len());
        if allowed == 0 {
            return Ok(BoundedRequestBody::Overflow {
                prefix: body,
                reader,
            });
        }
        let take = allowed.min(buffer.len());
        match reader.read(
            stream,
            &mut buffer[..take],
            max_header_size,
            max_header_count,
        )? {
            RequestBodyRead::Data(read) => {
                body.extend_from_slice(&buffer[..read]);
                if body.len() > limit {
                    return Ok(BoundedRequestBody::Overflow {
                        prefix: body,
                        reader,
                    });
                }
            }
            RequestBodyRead::End(trailers) => {
                return Ok(BoundedRequestBody::Complete { body, trailers });
            }
        }
    }
}

pub(crate) fn read_request_body_all<R: Read + ?Sized>(
    stream: &mut R,
    mut reader: RequestBodyReader,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<BodyAndTrailers> {
    let mut body = Vec::new();
    let mut buffer = [0u8; REQUEST_BODY_CHUNK_SIZE];
    loop {
        match reader.read(stream, &mut buffer, max_header_size, max_header_count)? {
            RequestBodyRead::Data(read) => {
                body.try_reserve(read).map_err(|_| {
                    io::Error::new(io::ErrorKind::OutOfMemory, "request body is too large")
                })?;
                body.extend_from_slice(&buffer[..read]);
            }
            RequestBodyRead::End(trailers) => return Ok((body, trailers)),
        }
    }
}
