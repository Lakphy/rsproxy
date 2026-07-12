use super::*;

pub(super) struct ResponseBody {
    pub(super) body: Vec<u8>,
    pub(super) trailers: Vec<(String, String)>,
}

pub(super) fn read_response_body<R: Read + ?Sized>(
    stream: &mut R,
    headers: &[(String, String)],
) -> io::Result<ResponseBody> {
    if has_chunked_transfer_encoding(headers) {
        return read_chunked_body(stream);
    }
    if let Some(len) = http::header(headers, "content-length").and_then(|v| v.parse::<usize>().ok())
    {
        let mut body = vec![0; len];
        stream.read_exact(&mut body)?;
        return Ok(ResponseBody {
            body,
            trailers: Vec::new(),
        });
    }

    let mut body = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                return Ok(ResponseBody {
                    body,
                    trailers: Vec::new(),
                });
            }
            Ok(n) => body.extend_from_slice(&buf[..n]),
            Err(err) if tls_close_notify_missing(&err) => {
                return Ok(ResponseBody {
                    body,
                    trailers: Vec::new(),
                });
            }
            Err(err) => return Err(err),
        }
    }
}

pub(super) fn stream_sse_response<W, R, F>(
    client: &mut W,
    upstream: &mut R,
    upstream_headers: &[(String, String)],
    trace_limit: usize,
    bytes_per_sec: Option<u64>,
    mut observe: F,
) -> io::Result<(u64, Vec<u8>, Vec<FrameRecord>)>
where
    W: Write + ?Sized,
    R: Read + ?Sized,
    F: FnMut(&[u8]),
{
    let mut capture = SseStreamCapture::new(trace_limit);
    let mut throttle = ThrottlePacer::new(bytes_per_sec);
    let mut response_bytes = 0u64;
    let mut buf = [0u8; 8192];

    if has_chunked_transfer_encoding(upstream_headers) {
        loop {
            let line = read_crlf_line(upstream)?;
            let size_hex = line.split(';').next().unwrap_or("").trim();
            let size = usize::from_str_radix(size_hex, 16).map_err(|_| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("invalid chunk size `{size_hex}`"),
                )
            })?;
            if size == 0 {
                loop {
                    let trailer = read_crlf_line(upstream)?;
                    if trailer.is_empty() {
                        capture.finish();
                        return Ok((response_bytes, capture.body_head, capture.frames));
                    }
                }
            }
            let mut chunk = vec![0u8; size];
            upstream.read_exact(&mut chunk)?;
            let mut crlf = [0u8; 2];
            upstream.read_exact(&mut crlf)?;
            if crlf != *b"\r\n" {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "chunk missing trailing CRLF",
                ));
            }
            if !write_streaming_payload(client, &chunk, &mut throttle)? {
                capture.finish();
                return Ok((response_bytes, capture.body_head, capture.frames));
            }
            response_bytes += chunk.len() as u64;
            capture.push(&chunk);
            observe(&chunk);
        }
    }

    if let Some(len) =
        http::header(upstream_headers, "content-length").and_then(|v| v.parse::<usize>().ok())
    {
        let mut remaining = len;
        while remaining > 0 {
            let take = remaining.min(buf.len());
            upstream.read_exact(&mut buf[..take])?;
            if !write_streaming_payload(client, &buf[..take], &mut throttle)? {
                break;
            }
            response_bytes += take as u64;
            capture.push(&buf[..take]);
            observe(&buf[..take]);
            remaining -= take;
        }
        capture.finish();
        return Ok((response_bytes, capture.body_head, capture.frames));
    }

    loop {
        match upstream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                if !write_streaming_payload(client, &buf[..n], &mut throttle)? {
                    break;
                }
                response_bytes += n as u64;
                capture.push(&buf[..n]);
                observe(&buf[..n]);
            }
            Err(err) if tls_close_notify_missing(&err) => break,
            Err(err) => return Err(err),
        }
    }
    capture.finish();
    Ok((response_bytes, capture.body_head, capture.frames))
}

pub(super) fn write_streaming_payload<W: Write + ?Sized>(
    client: &mut W,
    data: &[u8],
    throttle: &mut ThrottlePacer,
) -> io::Result<bool> {
    if data.is_empty() {
        return Ok(true);
    }
    match throttle.write(client, data) {
        Ok(()) => Ok(true),
        Err(err) if tunnel_end_error(&err) => Ok(false),
        Err(err) => Err(err),
    }
}

pub(super) struct SseStreamCapture {
    trace_limit: usize,
    body_head: Vec<u8>,
    frames: Vec<FrameRecord>,
    pending: String,
}

impl SseStreamCapture {
    fn new(trace_limit: usize) -> Self {
        Self {
            trace_limit,
            body_head: Vec::new(),
            frames: Vec::new(),
            pending: String::new(),
        }
    }

    fn push(&mut self, data: &[u8]) {
        let remaining = self.trace_limit.saturating_sub(self.body_head.len());
        if remaining > 0 {
            self.body_head.extend(data.iter().copied().take(remaining));
        }

        if self.frames.len() >= 512 {
            return;
        }
        self.pending.push_str(
            &String::from_utf8_lossy(data)
                .replace("\r\n", "\n")
                .replace('\r', "\n"),
        );
        while self.frames.len() < 512 {
            let Some(idx) = self.pending.find("\n\n") else {
                break;
            };
            let frame = self.pending[..idx].trim_end_matches('\n').to_string();
            self.pending.drain(..idx + 2);
            self.push_frame(frame);
        }
    }

    fn finish(&mut self) {
        if self.frames.len() < 512 && !self.pending.trim().is_empty() {
            self.push_frame(self.pending.trim_end_matches('\n').to_string());
        }
        self.pending.clear();
    }

    fn push_frame(&mut self, frame: String) {
        if frame.trim().is_empty() || self.frames.len() >= 512 {
            return;
        }
        self.frames.push(FrameRecord {
            direction: FrameDirection::ServerToClient,
            at_ms: rsproxy_trace::now_millis(),
            opcode: "sse".to_string(),
            fin: true,
            payload_len: frame.len() as u64,
            data_encoding: FrameDataEncoding::Utf8,
            data: frame.into_bytes(),
            truncated: false,
        });
    }
}

pub(super) fn tls_close_notify_missing(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::UnexpectedEof | io::ErrorKind::InvalidData
    ) && err.to_string().contains("close_notify")
}

pub(super) fn has_chunked_transfer_encoding(headers: &[(String, String)]) -> bool {
    http::header(headers, "transfer-encoding")
        .map(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case("chunked"))
        })
        .unwrap_or(false)
}

pub(super) fn read_chunked_body<R: Read + ?Sized>(stream: &mut R) -> io::Result<ResponseBody> {
    let mut body = Vec::new();
    let mut trailers = Vec::new();
    loop {
        let line = read_crlf_line(stream)?;
        let size_hex = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_hex, 16).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid chunk size `{size_hex}`"),
            )
        })?;
        if size == 0 {
            loop {
                let trailer = read_crlf_line(stream)?;
                if trailer.is_empty() {
                    return Ok(ResponseBody { body, trailers });
                }
                if let Some((name, value)) = trailer.split_once(':') {
                    let name = name.trim();
                    if !name.is_empty() {
                        trailers.push((name.to_string(), value.trim().to_string()));
                    }
                }
            }
        }
        let start = body.len();
        body.resize(start + size, 0);
        stream.read_exact(&mut body[start..])?;
        let mut crlf = [0u8; 2];
        stream.read_exact(&mut crlf)?;
        if crlf != *b"\r\n" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk missing trailing CRLF",
            ));
        }
    }
}

pub(super) fn read_crlf_line<R: Read + ?Sized>(stream: &mut R) -> io::Result<String> {
    let mut buf = Vec::with_capacity(32);
    let mut byte = [0u8; 1];
    loop {
        stream.read_exact(&mut byte)?;
        buf.push(byte[0]);
        if buf.ends_with(b"\r\n") {
            buf.truncate(buf.len() - 2);
            return Ok(String::from_utf8_lossy(&buf).to_string());
        }
        if buf.len() > 8192 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "chunk line too large",
            ));
        }
    }
}
