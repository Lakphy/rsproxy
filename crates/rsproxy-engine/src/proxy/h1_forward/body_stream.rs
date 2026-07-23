use super::*;
use bytes::Bytes;

const READ_BUFFER_SIZE: usize = 16 * 1024;

#[derive(Debug, PartialEq, Eq)]
enum BodyState {
    Empty,
    ContentLength(usize),
    ChunkSize,
    ChunkData(usize),
    ChunkTerminator,
    CloseDelimited,
    Done,
}

pub(super) struct H1BodyStream<R> {
    reader: R,
    state: BodyState,
    max_header_size: usize,
    max_header_count: usize,
    receive_started: Instant,
}

impl<R: Read> H1BodyStream<R> {
    pub(super) fn new(
        reader: R,
        method: &str,
        status: u16,
        headers: &[(String, String)],
        max_header_size: usize,
        max_header_count: usize,
        receive_started: Instant,
    ) -> io::Result<Self> {
        let body_allowed = http::response_has_framed_body(method, status);
        let state = if !body_allowed {
            BodyState::Empty
        } else if has_chunked_transfer_encoding(headers) {
            BodyState::ChunkSize
        } else if let Some(value) = http::header(headers, "content-length") {
            BodyState::ContentLength(value.parse::<usize>().map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid response content-length",
                )
            })?)
        } else if status == 205 {
            // A conforming 205 without explicit framing has no content. We do
            // consume explicitly framed malformed content so a persistent
            // upstream connection stays synchronized.
            BodyState::Empty
        } else {
            BodyState::CloseDelimited
        };
        Ok(Self {
            reader,
            state,
            max_header_size,
            max_header_count,
            receive_started,
        })
    }

    fn next_inner(&mut self) -> io::Result<Option<UpstreamBodyFrame>> {
        loop {
            match self.state {
                BodyState::Empty | BodyState::Done => {
                    self.state = BodyState::Done;
                    return Ok(None);
                }
                BodyState::ContentLength(0) => self.state = BodyState::Done,
                BodyState::ContentLength(remaining) => {
                    let mut buffer = vec![0; remaining.min(READ_BUFFER_SIZE)];
                    let read = self.reader.read(&mut buffer)?;
                    if read == 0 {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "response body ended before content-length was reached",
                        ));
                    }
                    buffer.truncate(read);
                    self.state = BodyState::ContentLength(remaining - read);
                    return Ok(Some(UpstreamBodyFrame::Data(Bytes::from(buffer))));
                }
                BodyState::ChunkSize => {
                    let line = read_crlf_line_bounded(&mut self.reader, self.max_header_size)?;
                    let raw_size = line.split(';').next().unwrap_or_default().trim();
                    let size = usize::from_str_radix(raw_size, 16).map_err(|_| {
                        io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size")
                    })?;
                    if size == 0 {
                        let trailers = read_trailers(
                            &mut self.reader,
                            self.max_header_size,
                            self.max_header_count,
                        )?;
                        self.state = BodyState::Done;
                        if trailers.is_empty() {
                            return Ok(None);
                        }
                        return Ok(Some(UpstreamBodyFrame::Trailers(trailers)));
                    }
                    self.state = BodyState::ChunkData(size);
                }
                BodyState::ChunkData(remaining) => {
                    let mut buffer = vec![0; remaining.min(READ_BUFFER_SIZE)];
                    self.reader.read_exact(&mut buffer)?;
                    self.state = if buffer.len() == remaining {
                        BodyState::ChunkTerminator
                    } else {
                        BodyState::ChunkData(remaining - buffer.len())
                    };
                    return Ok(Some(UpstreamBodyFrame::Data(Bytes::from(buffer))));
                }
                BodyState::ChunkTerminator => {
                    let mut delimiter = [0; 2];
                    self.reader.read_exact(&mut delimiter)?;
                    if delimiter != *b"\r\n" {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "chunk missing trailing CRLF",
                        ));
                    }
                    self.state = BodyState::ChunkSize;
                }
                BodyState::CloseDelimited => {
                    let mut buffer = vec![0; READ_BUFFER_SIZE];
                    match self.reader.read(&mut buffer) {
                        Ok(0) => self.state = BodyState::Done,
                        Ok(read) => {
                            buffer.truncate(read);
                            return Ok(Some(UpstreamBodyFrame::Data(Bytes::from(buffer))));
                        }
                        Err(error) if tls_close_notify_missing(&error) => {
                            self.state = BodyState::Done;
                        }
                        Err(error) => return Err(error),
                    }
                }
            }
        }
    }
}

impl<R: Read> ResponseBodyStream for H1BodyStream<R> {
    fn next_frame(&mut self) -> Option<io::Result<UpstreamBodyFrame>> {
        match self.next_inner() {
            Ok(frame) => frame.map(Ok),
            Err(error) => {
                self.state = BodyState::Done;
                Some(Err(stage_io_error("response_body", error)))
            }
        }
    }

    fn receive_ms(&self) -> Option<u64> {
        Some(duration_millis(self.receive_started.elapsed()))
    }
}

fn read_crlf_line_bounded<R: Read + ?Sized>(reader: &mut R, limit: usize) -> io::Result<String> {
    let mut bytes = Vec::with_capacity(32);
    let mut byte = [0; 1];
    loop {
        reader.read_exact(&mut byte)?;
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n") {
            bytes.truncate(bytes.len() - 2);
            return String::from_utf8(bytes)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid chunk line"));
        }
        if bytes.len() > limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk line exceeds response header limit",
            ));
        }
    }
}

fn read_trailers<R: Read + ?Sized>(
    reader: &mut R,
    max_header_size: usize,
    max_header_count: usize,
) -> io::Result<Vec<(String, String)>> {
    let mut trailers = Vec::new();
    let mut size = 0usize;
    loop {
        let line = read_crlf_line_bounded(reader, max_header_size)?;
        if line.is_empty() {
            return Ok(trailers);
        }
        size = size.saturating_add(line.len() + 2);
        if size > max_header_size || trailers.len() >= max_header_count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "response trailer limit exceeded",
            ));
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid trailer"))?;
        let name = name.trim();
        if name.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid empty trailer name",
            ));
        }
        trailers.push((name.to_string(), value.trim().to_string()));
    }
}

#[cfg(test)]
mod tests;
