use super::*;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};

fn request(origin: SocketAddr, method: &str) -> RawRequest {
    RawRequest {
        method: method.to_string(),
        target: format!("http://{origin}/fast"),
        version: "HTTP/1.1".to_string(),
        headers: vec![("Host".to_string(), origin.to_string())],
        body: Vec::new(),
        trailers: Vec::new(),
    }
}

fn run_response(
    method: &str,
    response: Vec<u8>,
    configure: impl FnOnce(&mut SharedState),
) -> (
    io::Result<ClientPersistence>,
    CapturedHttpResponse,
    SharedState,
) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap();
    let server_method = method.to_string();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        read_request_head(&mut reader, &server_method);
        writer.write_all(&response).unwrap();
        writer.flush().unwrap();
    });

    let mut state = test_state();
    state.config.storage = std::env::temp_dir().join(format!(
        "rsproxy-fast-h1-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    configure(&mut state);
    let mut capture = CapturedHttpResponse::default();
    let result = handle_http_stream(
        &mut capture,
        request(origin, method),
        state.clone(),
        test_connection_input(),
    );
    server.join().unwrap();
    (result, capture, state)
}

fn completed_session(state: &SharedState) -> Session {
    let sessions = state.trace.list(8);
    assert_eq!(sessions.len(), 1, "{sessions:#?}");
    sessions.into_iter().next().unwrap()
}

fn run_local_response(method: &str, rules: &str) -> (CapturedHttpResponse, SharedState) {
    let state = support::isolated_state("local-response", rules);
    let origin = "127.0.0.1:9".parse().unwrap();
    let mut capture = CapturedHttpResponse::default();
    handle_http_stream(
        &mut capture,
        request(origin, method),
        state.clone(),
        test_connection_input(),
    )
    .unwrap();
    (capture, state)
}

fn split_wire_response(bytes: &[u8]) -> (&[u8], &[u8]) {
    let boundary = bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("response must contain a complete head");
    (&bytes[..boundary + 4], &bytes[boundary + 4..])
}

#[test]
fn local_status_responses_obey_head_and_bodyless_status_semantics() {
    let (capture, state) = run_local_response("GET", "127.0.0.1 status(204)");
    let (head, body) = split_wire_response(&capture.bytes);
    let head = String::from_utf8_lossy(head);
    assert!(head.starts_with("HTTP/1.1 204 No Content\r\n"));
    assert!(!head.to_ascii_lowercase().contains("content-length:"));
    assert!(!head.to_ascii_lowercase().contains("transfer-encoding:"));
    assert!(body.is_empty());
    assert_eq!(completed_session(&state).response_bytes, 0);
    let _ = fs::remove_dir_all(&state.config.storage);

    let (capture, state) = run_local_response("HEAD", "127.0.0.1 status(200)");
    let (head, body) = split_wire_response(&capture.bytes);
    let head = String::from_utf8_lossy(head);
    assert!(head.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(head.to_ascii_lowercase().contains("content-length:"));
    assert!(body.is_empty());
    assert_eq!(completed_session(&state).response_bytes, 0);
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn local_mock_response_writer_owns_framing_headers() {
    let rules = concat!(
        "127.0.0.1 mock(status=200, header=Content-Length: 0, ",
        "header=Transfer-Encoding: chunked, body=unexpected)"
    );
    let (capture, state) = run_local_response("GET", rules);
    let (head, body) = split_wire_response(&capture.bytes);
    let head = String::from_utf8_lossy(head).to_ascii_lowercase();
    assert_eq!(head.matches("content-length:").count(), 1);
    assert!(head.contains("content-length: 10\r\n"));
    assert!(!head.contains("transfer-encoding:"));
    assert_eq!(body, b"unexpected");
    assert_eq!(completed_session(&state).response_bytes, 10);
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn empty_rule_plain_gets_use_the_fast_pool_and_keep_trace_contracts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap();
    let accepts = Arc::new(AtomicUsize::new(0));
    let server_accepts = Arc::clone(&accepts);
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        server_accepts.fetch_add(1, Ordering::Relaxed);
        let mut writer = stream.try_clone().unwrap();
        let mut reader = BufReader::new(stream);
        for _ in 0..2 {
            read_request_head(&mut reader, "GET");
            writer
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: keep-alive\r\n\r\nfast",
                )
                .unwrap();
        }
    });

    let mut state = test_state();
    state.config.storage = std::env::temp_dir().join(format!(
        "rsproxy-fast-h1-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    for _ in 0..2 {
        let mut capture = CapturedHttpResponse::default();
        handle_http_stream(
            &mut capture,
            request(origin, "GET"),
            state.clone(),
            test_connection_input(),
        )
        .unwrap();
        assert!(capture.bytes.ends_with(b"\r\n\r\nfast"));
    }

    server.join().unwrap();
    assert_eq!(accepts.load(Ordering::Relaxed), 1);
    let sessions = state.trace.list(8);
    assert_eq!(sessions.len(), 2);
    assert!(
        sessions.iter().all(|session| {
            session.flags.iter().any(|flag| flag == "h1-fast-path")
                && session.status == Some(200)
                && session.response_bytes == 4
        }),
        "{sessions:#?}"
    );
    assert!(sessions.iter().any(|session| {
        session
            .flags
            .iter()
            .any(|flag| flag == "h1-upstream-pool-miss")
    }));
    assert!(sessions.iter().any(|session| {
        session
            .flags
            .iter()
            .any(|flag| flag == "h1-upstream-pool-hit")
    }));
}

#[test]
fn large_fixed_response_streams_exact_bytes_and_trace_preview() {
    let body = vec![b'x'; 70 * 1024];
    let mut response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(&body);
    let (result, capture, state) = run_response("GET", response, |_| {});
    result.unwrap();
    assert!(capture.bytes.ends_with(&body));
    let session = completed_session(&state);
    assert_eq!(session.response_bytes, body.len() as u64);
    assert_eq!(session.res_body_head, body[..session.res_body_head.len()]);
    assert!(session.flags.iter().any(|flag| flag == "response-streamed"));
}

#[test]
fn chunked_response_preserves_chunks_declared_trailers_and_trace() {
    let response = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nTrailer: X-End\r\nConnection: keep-alive\r\n\r\n5\r\nhello\r\n6; note=yes\r\n world\r\n0\r\nX-End: done\r\n\r\n".to_vec();
    let (result, capture, state) = run_response("GET", response, |_| {});
    result.unwrap();
    assert!(
        capture
            .bytes
            .ends_with(b"5\r\nhello\r\n6\r\n world\r\n0\r\nX-End: done\r\n\r\n")
    );
    let session = completed_session(&state);
    assert_eq!(session.response_bytes, 11);
    assert_eq!(session.res_body_head, b"hello world");
    assert_eq!(
        session.res_trailers,
        vec![("X-End".to_string(), "done".to_string())]
    );
}

#[test]
fn close_delimited_response_forces_client_close_after_exact_body() {
    let response = b"HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nclose-delimited".to_vec();
    let (result, capture, state) = run_response("GET", response, |_| {});
    result.unwrap();
    let text = String::from_utf8(capture.bytes).unwrap();
    assert!(text.contains("Connection: close\r\n"));
    assert!(text.ends_with("close-delimited"));
    let session = completed_session(&state);
    assert_eq!(session.response_bytes, 15);
    assert!(session.flags.iter().any(|flag| flag == "response-streamed"));
}

#[test]
fn head_and_no_content_responses_never_consume_an_origin_body() {
    for (method, status) in [("HEAD", "200 OK"), ("GET", "204 No Content")] {
        let response =
            format!("HTTP/1.1 {status}\r\nContent-Length: 9\r\nConnection: keep-alive\r\n\r\n")
                .into_bytes();
        let (result, capture, state) = run_response(method, response, |_| {});
        result.unwrap();
        assert!(capture.bytes.ends_with(b"\r\n\r\n"));
        assert_eq!(completed_session(&state).response_bytes, 0);
    }
}

#[test]
fn reset_content_consumes_malformed_upstream_bytes_but_never_forwards_them() {
    let large = vec![b'x'; 70 * 1024];
    let mut fixed = format!(
        "HTTP/1.1 205 Reset Content\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
        large.len()
    )
    .into_bytes();
    fixed.extend_from_slice(&large);
    let chunked = b"HTTP/1.1 205 Reset Content\r\nTransfer-Encoding: chunked\r\nConnection: keep-alive\r\n\r\n9\r\ndiscarded\r\n0\r\n\r\n".to_vec();

    for response in [fixed, chunked] {
        let (result, capture, state) = run_response("GET", response, |_| {});
        result.unwrap();
        let (head, body) = split_wire_response(&capture.bytes);
        let head = String::from_utf8_lossy(head).to_ascii_lowercase();
        assert!(head.starts_with("http/1.1 205 reset content\r\n"));
        assert!(head.contains("content-length: 0\r\n"));
        assert!(!head.contains("transfer-encoding:"));
        assert!(body.is_empty());

        let session = completed_session(&state);
        assert_eq!(session.response_bytes, 0);
        assert!(
            session
                .flags
                .iter()
                .any(|flag| flag == "upstream-205-content-discarded")
        );
    }
}

#[test]
fn forbidden_and_connection_nominated_upstream_trailers_are_dropped() {
    let response = concat!(
        "HTTP/1.1 200 OK\r\n",
        "Transfer-Encoding: chunked\r\n",
        "Trailer: grpc-status, content-length, x-hop\r\n",
        "Connection: x-hop\r\n\r\n",
        "2\r\nok\r\n0\r\n",
        "grpc-status: 0\r\ncontent-length: 99\r\nx-hop: secret\r\n\r\n"
    )
    .as_bytes()
    .to_vec();
    let (result, capture, state) = run_response("GET", response, |_| {});

    result.unwrap();
    let text = String::from_utf8(capture.bytes)
        .unwrap()
        .to_ascii_lowercase();
    assert!(text.ends_with("0\r\ngrpc-status: 0\r\n\r\n"), "{text:?}");
    assert!(!text.contains("content-length: 99"));
    assert!(!text.contains("x-hop: secret"));
    let session = completed_session(&state);
    assert_eq!(
        session.res_trailers,
        vec![("grpc-status".to_string(), "0".to_string())]
    );
    assert!(
        session
            .flags
            .iter()
            .any(|flag| flag == "forbidden-upstream-trailer-dropped")
    );
}

#[test]
fn fixed_and_chunked_sse_responses_record_frames_and_force_close() {
    let body = b"data: one\n\ndata: two\n\n";
    let mut fixed = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    fixed.extend_from_slice(body);
    let (result, capture, state) = run_response("GET", fixed, |_| {});
    result.unwrap();
    assert!(capture.bytes.ends_with(body));
    let session = completed_session(&state);
    assert_eq!(session.kind, SessionKind::Sse);
    assert_eq!(session.frames.len(), 2);

    let chunked = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nTransfer-Encoding: chunked\r\n\r\nB\r\ndata: one\n\n\r\n0\r\nX-Ignored: yes\r\n\r\n".to_vec();
    let (result, capture, state) = run_response("GET", chunked, |_| {});
    result.unwrap();
    assert!(capture.bytes.ends_with(b"data: one\n\n"));
    let session = completed_session(&state);
    assert_eq!(session.kind, SessionKind::Sse);
    assert_eq!(session.frames.len(), 1);
}

#[test]
fn malformed_chunk_framing_and_trailers_fail_after_the_response_head() {
    let cases = [
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nnope\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n1\r\nxNO0\r\n\r\n".to_vec(),
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\ninvalid\r\n\r\n".to_vec(),
    ];
    for response in cases {
        let (result, capture, state) = run_response("GET", response, |_| {});
        assert_eq!(result.unwrap(), ClientPersistence::Close);
        assert!(capture.bytes.starts_with(b"HTTP/1.1 200 OK\r\n"));
        assert_eq!(
            String::from_utf8_lossy(&capture.bytes)
                .matches("HTTP/1.1")
                .count(),
            1
        );
        let session = completed_session(&state);
        assert_eq!(session.status, Some(200));
        assert!(
            session
                .error
                .as_deref()
                .unwrap()
                .contains("stage=response_body")
        );
        assert!(
            session
                .flags
                .iter()
                .any(|flag| flag == "upstream-response-body-error")
        );
    }

    let response =
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-One: 1\r\nX-Two: 2\r\n\r\n"
            .to_vec();
    let (result, capture, state) = run_response("GET", response, |state| {
        state.config.max_header_count = 1;
    });
    assert_eq!(result.unwrap(), ClientPersistence::Close);
    assert_eq!(
        String::from_utf8_lossy(&capture.bytes)
            .matches("HTTP/1.1")
            .count(),
        1
    );
    assert!(completed_session(&state).error.is_some());
}

fn read_request_head(reader: &mut BufReader<TcpStream>, method: &str) {
    let mut first = String::new();
    reader.read_line(&mut first).unwrap();
    assert!(first.starts_with(&format!("{method} /fast HTTP/1.1")));
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            return;
        }
    }
}
