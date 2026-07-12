use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ResponseState {
    Head,
    ContentLength(usize),
    ChunkSize,
    ChunkData(usize),
    ChunkTerminator,
    ChunkTrailers,
    CloseDelimited,
    Discard,
    Done,
    Failed,
}

pub(super) struct H2ResponseWriter {
    head_sender: Option<oneshot::Sender<io::Result<H2BridgeHead>>>,
    body_sender: Option<mpsc::Sender<io::Result<H2BridgeFrame>>>,
    state: ResponseState,
    buffer: Vec<u8>,
    method: String,
    max_header_size: usize,
    max_header_count: usize,
}

impl H2ResponseWriter {
    pub(super) fn new(
        method: &str,
        max_header_size: usize,
        max_header_count: usize,
        channel_capacity: usize,
    ) -> (Self, H2BridgeOutput) {
        let (head_sender, head) = oneshot::channel();
        let (body_sender, body) = mpsc::channel(channel_capacity);
        (
            Self {
                head_sender: Some(head_sender),
                body_sender: Some(body_sender),
                state: ResponseState::Head,
                buffer: Vec::new(),
                method: method.to_string(),
                max_header_size,
                max_header_count,
            },
            H2BridgeOutput { head, body },
        )
    }

    pub(super) fn finish(&mut self) -> io::Result<()> {
        self.drain()?;
        match self.state {
            ResponseState::Done => Ok(()),
            ResponseState::CloseDelimited => {
                if !self.buffer.is_empty() {
                    let data = Bytes::from(std::mem::take(&mut self.buffer));
                    self.send_body(H2BridgeFrame::Data(data))?;
                }
                self.close_body();
                self.state = ResponseState::Done;
                Ok(())
            }
            ResponseState::Discard => {
                self.buffer.clear();
                self.state = ResponseState::Done;
                Ok(())
            }
            ResponseState::Failed => Ok(()),
            ResponseState::Head => self.fail(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP/2 bridge completed before a response head was written",
            )),
            _ => self.fail(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "HTTP/2 bridge response body ended before framing completed",
            )),
        }
    }

    pub(super) fn fail_external(&mut self, error: &io::Error) {
        self.signal_error(clone_io_error(error));
    }

    fn drain(&mut self) -> io::Result<()> {
        loop {
            match self.state {
                ResponseState::Head => {
                    let Some(end) = find_bytes(&self.buffer, b"\r\n\r\n") else {
                        if self.buffer.len() > self.max_header_size.saturating_add(4) {
                            return self.fail(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "HTTP/2 bridge response header size limit exceeded",
                            ));
                        }
                        return Ok(());
                    };
                    self.parse_head(end + 4)?;
                }
                ResponseState::ContentLength(remaining) => {
                    if remaining == 0 {
                        self.close_body();
                        self.state = ResponseState::Done;
                        continue;
                    }
                    if self.buffer.is_empty() {
                        return Ok(());
                    }
                    let size = remaining.min(self.buffer.len());
                    let data = Bytes::from(self.buffer.drain(..size).collect::<Vec<_>>());
                    self.send_body(H2BridgeFrame::Data(data))?;
                    self.state = ResponseState::ContentLength(remaining - size);
                }
                ResponseState::ChunkSize => {
                    let Some(end) = find_bytes(&self.buffer, b"\r\n") else {
                        if self.buffer.len() > self.max_header_size {
                            return self.fail(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "HTTP/2 bridge chunk size line limit exceeded",
                            ));
                        }
                        return Ok(());
                    };
                    let line =
                        String::from_utf8(self.buffer.drain(..end).collect()).map_err(|_| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                "invalid chunk size encoding",
                            )
                        })?;
                    self.buffer.drain(..2);
                    let size =
                        usize::from_str_radix(line.split(';').next().unwrap_or("").trim(), 16)
                            .map_err(|_| {
                                io::Error::new(io::ErrorKind::InvalidData, "invalid chunk size")
                            })?;
                    self.state = if size == 0 {
                        ResponseState::ChunkTrailers
                    } else {
                        ResponseState::ChunkData(size)
                    };
                }
                ResponseState::ChunkData(remaining) => {
                    if self.buffer.is_empty() {
                        return Ok(());
                    }
                    let size = remaining.min(self.buffer.len());
                    let data = Bytes::from(self.buffer.drain(..size).collect::<Vec<_>>());
                    self.send_body(H2BridgeFrame::Data(data))?;
                    self.state = if size == remaining {
                        ResponseState::ChunkTerminator
                    } else {
                        ResponseState::ChunkData(remaining - size)
                    };
                }
                ResponseState::ChunkTerminator => {
                    if self.buffer.len() < 2 {
                        return Ok(());
                    }
                    if &self.buffer[..2] != b"\r\n" {
                        return self.fail(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid chunk terminator in bridged response",
                        ));
                    }
                    self.buffer.drain(..2);
                    self.state = ResponseState::ChunkSize;
                }
                ResponseState::ChunkTrailers => {
                    if self.buffer.starts_with(b"\r\n") {
                        self.buffer.drain(..2);
                        self.close_body();
                        self.state = ResponseState::Done;
                        continue;
                    }
                    let Some(end) = find_bytes(&self.buffer, b"\r\n\r\n") else {
                        if self.buffer.len() > self.max_header_size.saturating_add(4) {
                            return self.fail(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "HTTP/2 bridge response trailer size limit exceeded",
                            ));
                        }
                        return Ok(());
                    };
                    let bytes = self.buffer.drain(..end).collect::<Vec<_>>();
                    self.buffer.drain(..4);
                    let trailers = self.parse_trailers(&bytes)?;
                    if !trailers.is_empty() {
                        self.send_body(H2BridgeFrame::Trailers(trailers))?;
                    }
                    self.close_body();
                    self.state = ResponseState::Done;
                }
                ResponseState::CloseDelimited => {
                    if self.buffer.is_empty() {
                        return Ok(());
                    }
                    let data = Bytes::from(std::mem::take(&mut self.buffer));
                    self.send_body(H2BridgeFrame::Data(data))?;
                }
                ResponseState::Discard => {
                    self.buffer.clear();
                    return Ok(());
                }
                ResponseState::Done => {
                    if self.buffer.is_empty() {
                        return Ok(());
                    }
                    return self.fail(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "bytes written after bridged response completed",
                    ));
                }
                ResponseState::Failed => return Ok(()),
            }
        }
    }

    fn parse_head(&mut self, size: usize) -> io::Result<()> {
        let bytes = self.buffer.drain(..size).collect::<Vec<_>>();
        let mut cursor = Cursor::new(bytes);
        let head =
            http::read_response_head(&mut cursor, self.max_header_size, self.max_header_count)?;
        if head.status == 101 {
            return self.fail(io::Error::new(
                io::ErrorKind::Unsupported,
                "WebSocket over HTTP/2 is not supported",
            ));
        }
        let body_allowed = !self.method.eq_ignore_ascii_case("HEAD")
            && !(100..200).contains(&head.status)
            && !matches!(head.status, 204 | 304);
        let chunked = header_contains_token(&head.headers, "transfer-encoding", "chunked");
        let content_length = http::header(&head.headers, "content-length")
            .map(|value| {
                value.parse::<usize>().map_err(|_| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid response content-length",
                    )
                })
            })
            .transpose()?;
        let mut headers = head.headers;
        prepare_h2_client_response_headers(&mut headers, head.status, None);
        self.state = if !body_allowed {
            ResponseState::Discard
        } else if chunked {
            ResponseState::ChunkSize
        } else if let Some(length) = content_length {
            ResponseState::ContentLength(length)
        } else {
            ResponseState::CloseDelimited
        };
        self.send_head(H2BridgeHead {
            status: head.status,
            headers,
        })?;
        if self.state == ResponseState::Discard {
            self.close_body();
        }
        Ok(())
    }

    fn parse_trailers(&self, bytes: &[u8]) -> io::Result<Vec<(String, String)>> {
        let text = std::str::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid trailer encoding"))?;
        let mut trailers = Vec::new();
        let mut size = 0usize;
        for line in text.split("\r\n").filter(|line| !line.is_empty()) {
            if trailers.len() >= self.max_header_count {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP/2 bridge response trailer count limit exceeded",
                ));
            }
            let (name, value) = line.split_once(':').ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid bridged response trailer",
                )
            })?;
            size = size
                .saturating_add(name.len())
                .saturating_add(value.len())
                .saturating_add(4);
            if size > self.max_header_size {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "HTTP/2 bridge response trailer size limit exceeded",
                ));
            }
            trailers.push((name.trim().to_string(), value.trim().to_string()));
        }
        Ok(trailers)
    }

    fn send_head(&mut self, head: H2BridgeHead) -> io::Result<()> {
        let sender = self.head_sender.take().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "response head already sent")
        })?;
        sender.send(Ok(head)).map_err(|_| {
            self.body_sender.take();
            self.state = ResponseState::Failed;
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "HTTP/2 response stream was cancelled",
            )
        })
    }

    fn send_body(&mut self, frame: H2BridgeFrame) -> io::Result<()> {
        let Some(sender) = self.body_sender.as_ref() else {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "HTTP/2 response body is closed",
            ));
        };
        sender.blocking_send(Ok(frame)).map_err(|_| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                "HTTP/2 response body was cancelled",
            )
        })
    }

    fn close_body(&mut self) {
        self.body_sender.take();
    }

    fn fail(&mut self, error: io::Error) -> io::Result<()> {
        let returned = clone_io_error(&error);
        self.signal_error(error);
        Err(returned)
    }

    fn signal_error(&mut self, error: io::Error) {
        if let Some(sender) = self.head_sender.take() {
            let _ = sender.send(Err(error));
        } else if let Some(sender) = self.body_sender.take() {
            let _ = sender.blocking_send(Err(error));
        }
        self.state = ResponseState::Failed;
    }
}

impl Write for H2ResponseWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.state == ResponseState::Failed {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "HTTP/2 response bridge has failed",
            ));
        }
        self.buffer.extend_from_slice(buffer);
        self.drain()?;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.drain()
    }
}

fn header_contains_token(headers: &[(String, String)], name: &str, token: &str) -> bool {
    headers
        .iter()
        .filter(|(seen, _)| seen.eq_ignore_ascii_case(name))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .any(|seen| seen.eq_ignore_ascii_case(token))
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn clone_io_error(error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), error.to_string())
}
