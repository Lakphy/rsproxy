use super::support::wait_for_trace_stats;
use super::*;
use bytes::Bytes;

fn request() -> RawRequest {
    RawRequest {
        method: "GET".to_string(),
        target: "/large".to_string(),
        version: "HTTP/1.1".to_string(),
        headers: vec![("Host".to_string(), "example.test".to_string())],
        body: Vec::new(),
        trailers: Vec::new(),
    }
}

fn streamed_response(body: UpstreamBody) -> UpstreamH2Response {
    UpstreamH2Response {
        status: 200,
        headers: vec![
            ("content-type".to_string(), "text/plain".to_string()),
            ("trailer".to_string(), "x-origin-end".to_string()),
        ],
        body,
        reused_connection: true,
        pool_wait_ms: 3,
        request_send_ms: 0,
        ttfb_ms: 1,
    }
}

fn captured_body(bytes: Vec<u8>) -> ResponseBody {
    let mut cursor = Cursor::new(bytes);
    let head = http::read_response_head(&mut cursor, 16 * 1024, 64).unwrap();
    read_response_body(&mut cursor, &head.headers).unwrap()
}

fn finish_response<W: WsIo + Send>(
    client: &mut W,
    request: &RawRequest,
    meta: &RequestMeta,
    state: &SharedState,
    trace_id: u64,
    response: UpstreamH2Response,
) -> io::Result<ForwardResult> {
    let rules = state.rules.snapshot();
    finish_h2_response_with_context(
        client,
        ResponseContext {
            request,
            meta,
            state,
            trace_id,
            upstream_addr: "example.test:443".to_string(),
            client_connection: ClientPersistence::KeepAlive,
            deadline: request_deadline(),
        },
        &rules.compiled,
        response,
        false,
    )
}

#[test]
fn small_fixed_length_response_keeps_framing_and_uses_buffered_completion() {
    let state = test_state();
    let request = request();
    let mut capture = CapturedHttpResponse::default();
    let response = UpstreamH2Response {
        status: 200,
        headers: vec![
            ("content-type".to_string(), "text/plain".to_string()),
            ("content-length".to_string(), "4".to_string()),
        ],
        body: UpstreamBody::from_collected(b"body".to_vec(), Vec::new()),
        reused_connection: true,
        pool_wait_ms: 0,
        request_send_ms: 0,
        ttfb_ms: 0,
    };

    let request_meta = meta("https://example.test/large");
    let result =
        finish_response(&mut capture, &request, &request_meta, &state, 0, response).unwrap();

    assert!(!result.flags.contains(&"response-streamed".to_string()));
    let output = String::from_utf8(capture.bytes).unwrap();
    assert!(output.contains("content-length: 4\r\n"));
    assert!(!output.to_ascii_lowercase().contains("transfer-encoding"));
    assert!(output.ends_with("\r\n\r\nbody"));
}

#[test]
fn body_rewrite_collects_only_when_the_payload_fits_the_limit() {
    let mut state = test_state();
    state.config.body_buffer_limit = 16;
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        RuleSet::parse(
            "default",
            "example.test res.body.append(\"!\") res.trailer(x-rule-end: yes)",
        )
        .unwrap(),
    );
    let request = request();
    let mut capture = CapturedHttpResponse::default();

    let request_meta = meta("https://example.test/large");
    let result = finish_response(
        &mut capture,
        &request,
        &request_meta,
        &state,
        0,
        streamed_response(UpstreamBody::from_collected(
            b"body".to_vec(),
            vec![("x-origin-end".to_string(), "done".to_string())],
        )),
    )
    .unwrap();

    assert!(!result.flags.contains(&"response-streamed".to_string()));
    assert_eq!(result.response_bytes, 5);
    assert_eq!(
        http::header(&result.res_trailers, "x-rule-end"),
        Some("yes")
    );
    assert_eq!(captured_body(capture.bytes).body, b"body!");
}

#[test]
fn body_rewrite_limit_falls_back_to_complete_unmodified_stream() {
    let mut state = test_state();
    state.config.body_buffer_limit = 4;
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        RuleSet::parse(
            "default",
            "example.test res.body.append(\"!\") res.trailer(x-rule-end: yes)",
        )
        .unwrap(),
    );
    let request = request();
    let mut capture = CapturedHttpResponse::default();

    let request_meta = meta("https://example.test/large");
    let result = finish_response(
        &mut capture,
        &request,
        &request_meta,
        &state,
        0,
        streamed_response(UpstreamBody::from_collected(
            b"abcdefgh".to_vec(),
            vec![("x-origin-end".to_string(), "done".to_string())],
        )),
    )
    .unwrap();

    assert!(result.flags.contains(&"response-streamed".to_string()));
    assert!(
        result
            .flags
            .contains(&"body-rewrite-skipped-limit".to_string())
    );
    assert_eq!(result.response_bytes, 8);
    assert_eq!(result.body_head, b"abcdefgh");
    assert_eq!(
        http::header(&result.res_trailers, "x-origin-end"),
        Some("done")
    );
    assert_eq!(
        http::header(&result.res_trailers, "x-rule-end"),
        Some("yes")
    );
    assert_eq!(captured_body(capture.bytes).body, b"abcdefgh");
}

#[test]
fn upstream_body_error_after_headers_closes_without_a_second_response() {
    let state = test_state();
    let request = request();
    let (sender, body, receive_timer) = rsproxy_net::test_timed_upstream_body_channel();
    sender
        .try_send(Ok(UpstreamBodyFrame::Data(Bytes::from_static(b"partial"))))
        .unwrap();
    sender
        .try_send(Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "origin reset",
        )))
        .unwrap();
    receive_timer.finish();
    drop(sender);
    let mut capture = CapturedHttpResponse::default();

    let request_meta = meta("https://example.test/large");
    let result = finish_response(
        &mut capture,
        &request,
        &request_meta,
        &state,
        0,
        streamed_response(body),
    )
    .unwrap();

    assert_eq!(result.status, 200);
    assert_eq!(result.client_connection, ClientPersistence::Close);
    assert!(result.error.as_deref().unwrap().contains("origin reset"));
    assert!(
        result
            .flags
            .contains(&"upstream-response-body-error".to_string())
    );
    assert!(result.response_receive_ms.is_some());
    let output = String::from_utf8_lossy(&capture.bytes);
    assert!(output.starts_with("HTTP/1.1 200 OK\r\n"));
    assert_eq!(output.matches("HTTP/1.1").count(), 1);
    assert!(!output.ends_with("0\r\n\r\n"));
}

#[test]
fn streamed_response_body_is_visible_to_collector_before_session_end() {
    let mut state = test_state();
    state.config.trace_body_limit = 8 * 1024;
    let trace_id = state.trace.start(rsproxy_trace::SessionStart {
        kind: SessionKind::Http,
        started_ms: rsproxy_trace::now_millis(),
        method: "GET".to_string(),
        url: "https://example.test/large".to_string(),
        client: "test-client".to_string(),
    });
    let started_memory = state.trace.stats().pending_memory_bytes;
    let (sender, body) = UpstreamBody::channel();
    let worker_state = state.clone();
    let worker = thread::spawn(move || {
        let mut client = CountingClient::default();
        let request = request();
        let request_meta = meta("https://example.test/large");
        finish_response(
            &mut client,
            &request,
            &request_meta,
            &worker_state,
            trace_id,
            streamed_response(body),
        )
        .unwrap()
    });

    let headers = wait_for_trace_stats(&state.trace, |stats| {
        stats.pending_memory_bytes >= started_memory.saturating_add(1)
    });
    sender
        .blocking_send(Ok(UpstreamBodyFrame::Data(Bytes::from(vec![b'x'; 4096]))))
        .unwrap();
    let body_pending = wait_for_trace_stats(&state.trace, |stats| {
        stats.pending_memory_bytes >= headers.pending_memory_bytes.saturating_add(4096)
    });
    assert_eq!(body_pending.pending_sessions, 1);
    assert!(state.trace.list(1).is_empty());

    drop(sender);
    let result = worker.join().unwrap();
    assert_eq!(result.response_bytes, 4096);
    assert_eq!(result.body_head, vec![b'x'; 4096]);
    assert!(state.trace.abort(trace_id));
    assert_eq!(state.trace.stats().pending_sessions, 0);
}

#[test]
fn streamed_response_records_receive_time_independently_from_request_send() {
    let state = test_state();
    let request = request();
    let (sender, body, receive_timer) = rsproxy_net::test_timed_upstream_body_channel();
    let producer = thread::spawn(move || {
        sender
            .blocking_send(Ok(UpstreamBodyFrame::Data(Bytes::from_static(b"first"))))
            .unwrap();
        thread::sleep(Duration::from_millis(60));
        sender
            .blocking_send(Ok(UpstreamBodyFrame::Data(Bytes::from_static(b"second"))))
            .unwrap();
        receive_timer.finish();
    });
    let mut capture = CapturedHttpResponse::default();

    let request_meta = meta("https://example.test/large");
    let result = finish_response(
        &mut capture,
        &request,
        &request_meta,
        &state,
        0,
        streamed_response(body),
    )
    .unwrap();
    producer.join().unwrap();

    assert_eq!(result.request_send_ms, Some(0));
    assert!(result.response_receive_ms.unwrap() >= 50);
    assert_eq!(result.response_bytes, 11);
    assert_eq!(captured_body(capture.bytes).body, b"firstsecond");
}

#[derive(Default)]
struct CountingClient {
    total: u64,
    max_write: usize,
    prefix: Vec<u8>,
}

impl Read for CountingClient {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Ok(0)
    }
}

impl Write for CountingClient {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.total = self.total.saturating_add(data.len() as u64);
        self.max_write = self.max_write.max(data.len());
        let remaining = 4096usize.saturating_sub(self.prefix.len());
        self.prefix.extend(data.iter().copied().take(remaining));
        Ok(data.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl WsIo for CountingClient {
    fn set_ws_nonblocking(&mut self, _nonblocking: bool) -> io::Result<()> {
        Ok(())
    }

    fn shutdown_ws(&mut self, _how: Shutdown) -> io::Result<()> {
        Ok(())
    }

    fn set_request_read_timeout(&mut self, _timeout: Option<Duration>) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn large_response_stream_keeps_only_the_trace_prefix() {
    const CHUNK_SIZE: usize = 16 * 1024;
    const CHUNKS: usize = 2048;
    let mut state = test_state();
    state.config.trace_body_limit = 1024;
    let request = request();
    let (sender, body) = UpstreamBody::channel();
    let producer = thread::spawn(move || {
        let chunk = Bytes::from(vec![b'x'; CHUNK_SIZE]);
        for _ in 0..CHUNKS {
            sender
                .blocking_send(Ok(UpstreamBodyFrame::Data(chunk.clone())))
                .unwrap();
        }
    });
    let mut client = CountingClient::default();

    let request_meta = meta("https://example.test/large");
    let result = finish_response(
        &mut client,
        &request,
        &request_meta,
        &state,
        0,
        streamed_response(body),
    )
    .unwrap();
    producer.join().unwrap();

    assert_eq!(result.response_bytes, (CHUNK_SIZE * CHUNKS) as u64);
    assert_eq!(result.body_head.len(), 1024);
    assert!(client.max_write <= CHUNK_SIZE);
    assert!(client.total > result.response_bytes);
    assert!(client.prefix.starts_with(b"HTTP/1.1 200 OK\r\n"));
}
