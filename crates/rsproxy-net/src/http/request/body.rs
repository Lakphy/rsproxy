use std::io::{self, Read};

use super::RequestBodyFraming;

mod collect;
mod trailers;

pub(crate) use collect::read_request_body_all;
pub use collect::read_request_body_bounded;
pub use trailers::validate_request_trailers;

use trailers::{read_crlf_line_limited, read_trailers};

#[derive(Debug)]
/// Outcome of buffering a request body under a byte limit.
pub enum BoundedRequestBody {
    /// The body ended before exceeding the configured limit.
    Complete {
        /// Decoded body bytes.
        body: Vec<u8>,
        /// Validated terminal trailers.
        trailers: Vec<(String, String)>,
    },
    /// The byte limit was exceeded and decoding can continue from `reader`.
    Overflow {
        /// Leading bytes retained, including at most one byte beyond the limit.
        prefix: Vec<u8>,
        /// Stateful decoder positioned immediately after `prefix`.
        reader: RequestBodyReader,
    },
}

#[derive(Debug)]
/// Incremental decoder for content-length and chunked HTTP/1 request bodies.
pub struct RequestBodyReader {
    framing: RequestBodyFraming,
    state: RequestBodyState,
}

#[derive(Debug)]
enum RequestBodyState {
    Finished,
    ContentLength { remaining: usize },
    Chunked { remaining: usize },
}

#[derive(Debug, PartialEq, Eq)]
/// Progress reported by one incremental request-body read.
pub enum RequestBodyRead {
    /// Number of decoded bytes written into the caller's buffer.
    Data(usize),
    /// End of body with any validated chunked trailers.
    End(Vec<(String, String)>),
}

impl RequestBodyReader {
    /// Starts a decoder at the beginning of a body with known framing.
    pub fn new(framing: RequestBodyFraming) -> Self {
        let state = match framing {
            RequestBodyFraming::None | RequestBodyFraming::ContentLength(0) => {
                RequestBodyState::Finished
            }
            RequestBodyFraming::ContentLength(remaining) => {
                RequestBodyState::ContentLength { remaining }
            }
            RequestBodyFraming::Chunked => RequestBodyState::Chunked { remaining: 0 },
        };
        Self { framing, state }
    }

    /// Returns the framing selected when this decoder was created.
    pub fn framing(&self) -> RequestBodyFraming {
        self.framing
    }

    /// Reads one decoded fragment, enforcing limits on chunk metadata and trailers.
    pub fn read<R: Read + ?Sized>(
        &mut self,
        stream: &mut R,
        buffer: &mut [u8],
        max_header_size: usize,
        max_header_count: usize,
    ) -> io::Result<RequestBodyRead> {
        if buffer.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "request body read buffer must not be empty",
            ));
        }
        match &mut self.state {
            RequestBodyState::Finished => Ok(RequestBodyRead::End(Vec::new())),
            RequestBodyState::ContentLength { remaining } => {
                let take = (*remaining).min(buffer.len());
                let read = stream.read(&mut buffer[..take])?;
                if read == 0 {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "connection closed before request body completed",
                    ));
                }
                *remaining -= read;
                if *remaining == 0 {
                    self.state = RequestBodyState::Finished;
                }
                Ok(RequestBodyRead::Data(read))
            }
            RequestBodyState::Chunked { remaining } => {
                if *remaining == 0 {
                    let size_line = read_crlf_line_limited(stream, max_header_size)?;
                    let size_text = size_line.split(';').next().unwrap_or("").trim();
                    let size = usize::from_str_radix(size_text, 16).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size in request")
                    })?;
                    if size == 0 {
                        let trailers = read_trailers(stream, max_header_size, max_header_count)?;
                        self.state = RequestBodyState::Finished;
                        return Ok(RequestBodyRead::End(trailers));
                    }
                    *remaining = size;
                }
                let take = (*remaining).min(buffer.len());
                stream.read_exact(&mut buffer[..take])?;
                *remaining -= take;
                if *remaining == 0 {
                    let mut crlf = [0u8; 2];
                    stream.read_exact(&mut crlf)?;
                    if crlf != *b"\r\n" {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid chunk terminator in request",
                        ));
                    }
                }
                Ok(RequestBodyRead::Data(take))
            }
        }
    }
}
