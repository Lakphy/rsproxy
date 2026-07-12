use super::*;

const REQUEST_STREAM_CHUNK_SIZE: usize = 16 * 1024;

pub(super) struct StreamingRequestBody {
    pub(super) prefix: Vec<u8>,
    pub(super) reader: http::RequestBodyReader,
    pub(super) exceeded_buffer_limit: bool,
}

impl StreamingRequestBody {
    pub(super) fn overflow(
        prefix: Vec<u8>,
        reader: http::RequestBodyReader,
        body_rules_skipped: bool,
    ) -> Self {
        Self {
            prefix,
            reader,
            exceeded_buffer_limit: body_rules_skipped,
        }
    }

    pub(super) fn framing(&self) -> http::RequestBodyFraming {
        self.reader.framing()
    }
}

pub(super) struct RequestStreamSummary {
    pub(super) bytes: u64,
    pub(super) body_head: Vec<u8>,
    pub(super) trailers: Vec<(String, String)>,
    pub(super) exceeded_buffer_limit: bool,
    pub(super) completed: bool,
}

pub(super) struct RequestRelayConfig<'a> {
    pub(super) trace_limit: usize,
    pub(super) bytes_per_sec: Option<u64>,
    pub(super) max_header_size: usize,
    pub(super) max_header_count: usize,
    pub(super) deadline: RequestDeadline,
    pub(super) trace: Option<(&'a rsproxy_trace::TraceStore, u64)>,
}

pub(super) fn request_expects_continue(request: &RawRequest) -> bool {
    header_contains_token(&request.headers, "expect", "100-continue")
}

pub(super) fn is_client_request_body_error(error: &io::Error) -> bool {
    error.to_string().starts_with("stage=client_request_body:")
}

pub(super) fn read_request_body_bounded_with_deadline<W: WsIo + ?Sized>(
    client: &mut W,
    framing: http::RequestBodyFraming,
    limit: usize,
    max_header_size: usize,
    max_header_count: usize,
    deadline: RequestDeadline,
) -> io::Result<http::BoundedRequestBody> {
    let result = {
        let mut input = ClientDeadlineReader::new(client, deadline);
        http::read_request_body_bounded(
            &mut input,
            framing,
            limit,
            max_header_size,
            max_header_count,
        )
        .map_err(|error| stage_io_error("client_request_body", error))
    };
    restore_request_timeout(client, result)
}

pub(super) fn relay_request_body<W: WsIo + ?Sized>(
    client: &mut W,
    upstream: &mut DeadlineIo<'_>,
    mut request_body: StreamingRequestBody,
    config: RequestRelayConfig<'_>,
) -> io::Result<RequestStreamSummary> {
    let RequestRelayConfig {
        trace_limit,
        bytes_per_sec,
        max_header_size,
        max_header_count,
        deadline,
        trace,
    } = config;
    let chunked = request_body.framing() == http::RequestBodyFraming::Chunked;
    let exceeded_buffer_limit = request_body.exceeded_buffer_limit;
    let result = (|| {
        let mut trace = trace.map(|(store, id)| {
            BodyTraceEmitter::new(
                store,
                id,
                rsproxy_trace::BodyDirection::Request,
                trace_limit,
            )
        });
        let mut input = ClientDeadlineReader::new(client, deadline);
        let mut summary = RequestStreamSummary {
            bytes: 0,
            body_head: Vec::with_capacity(trace_limit.min(64 * 1024)),
            trailers: Vec::new(),
            exceeded_buffer_limit,
            completed: true,
        };
        let mut throttle = ThrottlePacer::new(bytes_per_sec);
        if !request_body.prefix.is_empty() {
            observe_request_data(&mut summary, &request_body.prefix, trace_limit, &mut trace);
            write_request_data(
                upstream,
                &request_body.prefix,
                chunked,
                &mut throttle,
                deadline,
            )?;
        }

        let mut buffer = [0u8; REQUEST_STREAM_CHUNK_SIZE];
        loop {
            match request_body
                .reader
                .read(&mut input, &mut buffer, max_header_size, max_header_count)
                .map_err(|error| stage_io_error("client_request_body", error))?
            {
                http::RequestBodyRead::Data(read) => {
                    observe_request_data(&mut summary, &buffer[..read], trace_limit, &mut trace);
                    write_request_data(
                        upstream,
                        &buffer[..read],
                        chunked,
                        &mut throttle,
                        deadline,
                    )?;
                }
                http::RequestBodyRead::End(trailers) => {
                    summary.trailers = trailers;
                    finish_request_body(upstream, chunked, &summary.trailers)?;
                    break Ok(summary);
                }
            }
        }
    })();
    restore_request_timeout(client, result)
}

pub(super) fn relay_request_body_to_h2<W: WsIo + ?Sized>(
    client: &mut W,
    upstream: &mut StreamingH2Request,
    mut request_body: StreamingRequestBody,
    config: RequestRelayConfig<'_>,
) -> io::Result<RequestStreamSummary> {
    let RequestRelayConfig {
        trace_limit,
        max_header_size,
        max_header_count,
        deadline,
        trace,
        ..
    } = config;
    let exceeded_buffer_limit = request_body.exceeded_buffer_limit;
    let result = (|| {
        let mut trace = trace.map(|(store, id)| {
            BodyTraceEmitter::new(
                store,
                id,
                rsproxy_trace::BodyDirection::Request,
                trace_limit,
            )
        });
        let mut input = ClientDeadlineReader::new(client, deadline);
        let mut summary = RequestStreamSummary {
            bytes: 0,
            body_head: Vec::with_capacity(trace_limit.min(64 * 1024)),
            trailers: Vec::new(),
            exceeded_buffer_limit,
            completed: true,
        };
        if !request_body.prefix.is_empty() {
            let prefix = bytes::Bytes::from(std::mem::take(&mut request_body.prefix));
            observe_request_bytes(&mut summary, &prefix, trace_limit, &mut trace);
            if !upstream.send_data(prefix, deadline)? {
                summary.completed = false;
                return Ok(summary);
            }
        }

        let mut buffer = [0u8; REQUEST_STREAM_CHUNK_SIZE];
        loop {
            let next = request_body
                .reader
                .read(&mut input, &mut buffer, max_header_size, max_header_count)
                .map_err(|error| stage_io_error("client_request_body", error));
            let next = match next {
                Ok(next) => next,
                Err(error) => {
                    let _ = upstream.send_error(&error, deadline);
                    break Err(error);
                }
            };
            match next {
                http::RequestBodyRead::Data(read) => {
                    let data = bytes::Bytes::copy_from_slice(&buffer[..read]);
                    observe_request_bytes(&mut summary, &data, trace_limit, &mut trace);
                    if !upstream.send_data(data, deadline)? {
                        summary.completed = false;
                        break Ok(summary);
                    }
                }
                http::RequestBodyRead::End(trailers) => {
                    summary.trailers = trailers;
                    if !summary.trailers.is_empty()
                        && !upstream.send_trailers(summary.trailers.clone(), deadline)?
                    {
                        summary.completed = false;
                    }
                    break Ok(summary);
                }
            }
        }
    })();
    upstream.close_body();
    restore_request_timeout(client, result)
}

fn restore_request_timeout<T, W: WsIo + ?Sized>(
    client: &mut W,
    result: io::Result<T>,
) -> io::Result<T> {
    let restored = client.set_request_read_timeout(None);
    match result {
        Ok(value) => {
            restored?;
            Ok(value)
        }
        Err(error) => Err(error),
    }
}

struct ClientDeadlineReader<'a, W: WsIo + ?Sized> {
    client: &'a mut W,
    deadline: RequestDeadline,
}

impl<'a, W: WsIo + ?Sized> ClientDeadlineReader<'a, W> {
    fn new(client: &'a mut W, deadline: RequestDeadline) -> Self {
        Self { client, deadline }
    }
}

impl<W: WsIo + ?Sized> Read for ClientDeadlineReader<'_, W> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let remaining = self.deadline.remaining()?;
        self.client.set_request_read_timeout(Some(remaining))?;
        self.client.read(buffer).map_err(|error| {
            if matches!(
                error.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ) {
                self.deadline.timeout_error()
            } else {
                error
            }
        })
    }
}

fn observe_request_data(
    summary: &mut RequestStreamSummary,
    data: &[u8],
    trace_limit: usize,
    trace: &mut Option<BodyTraceEmitter<'_>>,
) {
    summary.bytes = summary.bytes.saturating_add(data.len() as u64);
    let remaining = trace_limit.saturating_sub(summary.body_head.len());
    summary
        .body_head
        .extend(data.iter().copied().take(remaining));
    if let Some(trace) = trace {
        trace.observe_slice(data);
    }
}

fn observe_request_bytes(
    summary: &mut RequestStreamSummary,
    data: &bytes::Bytes,
    trace_limit: usize,
    trace: &mut Option<BodyTraceEmitter<'_>>,
) {
    summary.bytes = summary.bytes.saturating_add(data.len() as u64);
    let remaining = trace_limit.saturating_sub(summary.body_head.len());
    summary
        .body_head
        .extend(data.iter().copied().take(remaining));
    if let Some(trace) = trace {
        trace.observe_bytes(data);
    }
}

fn write_request_data(
    upstream: &mut DeadlineIo<'_>,
    data: &[u8],
    chunked: bool,
    throttle: &mut ThrottlePacer,
    deadline: RequestDeadline,
) -> io::Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if chunked {
        write!(upstream, "{:X}\r\n", data.len())
            .map_err(|error| stage_io_error("request_write", error))?;
    }
    throttle
        .write_until(upstream, data, deadline)
        .map_err(|error| stage_io_error("request_write", error))?;
    if chunked {
        upstream
            .write_all(b"\r\n")
            .map_err(|error| stage_io_error("request_write", error))?;
    }
    Ok(())
}

fn finish_request_body(
    upstream: &mut DeadlineIo<'_>,
    chunked: bool,
    trailers: &[(String, String)],
) -> io::Result<()> {
    if chunked {
        upstream
            .write_all(b"0\r\n")
            .map_err(|error| stage_io_error("request_write", error))?;
        for (name, value) in trailers {
            write!(upstream, "{name}: {value}\r\n")
                .map_err(|error| stage_io_error("request_write", error))?;
        }
        upstream
            .write_all(b"\r\n")
            .map_err(|error| stage_io_error("request_write", error))?;
    }
    upstream
        .flush()
        .map_err(|error| stage_io_error("request_write", error))
}
