use super::super::*;

#[test]
fn connect_proxy_setup_uses_request_total_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0u8; 512];
        let _ = stream.read(&mut request);
        std::thread::sleep(Duration::from_millis(100));
    });
    let route = UpstreamRoute::HttpProxy {
        proxy_host: addr.ip().to_string(),
        proxy_port: addr.port(),
        target_host: "origin.test".to_string(),
        target_port: 443,
    };
    let state = test_state();
    let deadline = RequestDeadline::new(Duration::from_millis(40)).unwrap();
    let error =
        match connect_tunnel_upstream(&route, &state, &mut NetworkTimings::default(), deadline) {
            Err(error) => error,
            Ok(_) => panic!("silent CONNECT proxy unexpectedly completed setup"),
        };

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "stage=request_total: timeout after 40ms");
    worker.join().unwrap();
}

#[test]
fn dns_timeout_classification_does_not_include_lookup_failures() {
    let timeout = io::Error::new(
        io::ErrorKind::TimedOut,
        "stage=dns: timeout after 40ms resolving stalled.test",
    );
    assert!(is_upstream_dns_timeout(&timeout));
    assert!(!is_upstream_tcp_connect_timeout(&timeout));

    let not_found = io::Error::new(
        io::ErrorKind::NotFound,
        "stage=dns: failed to resolve missing.test: no records found",
    );
    assert!(!is_upstream_dns_timeout(&not_found));
}

#[test]
fn manual_ttfb_deadline_excludes_response_body_wait() {
    let silent_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let silent_addr = silent_listener.local_addr().unwrap();
    let silent_worker = std::thread::spawn(move || {
        let (_stream, _) = silent_listener.accept().unwrap();
        std::thread::sleep(Duration::from_millis(100));
    });
    let mut silent = UpstreamStream::Tcp(TcpStream::connect(silent_addr).unwrap());
    let mut timings = NetworkTimings::default();
    let started = Instant::now();
    let error = read_response_head_with_ttfb(
        &mut silent,
        4096,
        32,
        Duration::from_millis(40),
        request_deadline(),
        &mut timings,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "stage=ttfb: timeout after 40ms");
    assert!(is_upstream_ttfb_timeout(&error));
    assert!(started.elapsed() >= Duration::from_millis(30));
    silent_worker.join().unwrap();

    let body_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let body_addr = body_listener.local_addr().unwrap();
    let body_worker = std::thread::spawn(move || {
        let (mut stream, _) = body_listener.accept().unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
            .unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(80));
        stream.write_all(b"ok").unwrap();
    });
    let mut body_stream = UpstreamStream::Tcp(TcpStream::connect(body_addr).unwrap());
    let mut timings = NetworkTimings::default();
    let head = read_response_head_with_ttfb(
        &mut body_stream,
        4096,
        32,
        Duration::from_millis(40),
        request_deadline(),
        &mut timings,
    )
    .unwrap();
    let body_started = Instant::now();
    let response = read_response_body(&mut body_stream, &head.headers).unwrap();
    assert_eq!(response.body, b"ok");
    assert!(body_started.elapsed() >= Duration::from_millis(60));
    assert!(timings.ttfb_ms < 40);
    body_worker.join().unwrap();
}

#[test]
fn manual_response_body_uses_request_total_deadline() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let worker = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n")
            .unwrap();
        stream.flush().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let _ = stream.write_all(b"ok");
    });
    let mut stream = UpstreamStream::Tcp(TcpStream::connect(addr).unwrap());
    let deadline = RequestDeadline::new(Duration::from_millis(40)).unwrap();
    let mut timings = NetworkTimings::default();
    let head = read_response_head_with_ttfb(
        &mut stream,
        4096,
        32,
        Duration::from_secs(1),
        deadline,
        &mut timings,
    )
    .unwrap();
    let started = Instant::now();
    let result = {
        let mut io = DeadlineIo::new(&mut stream, deadline);
        read_response_body(&mut io, &head.headers)
    };
    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("slow response body unexpectedly completed"),
    };

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "stage=request_total: timeout after 40ms");
    assert!(started.elapsed() >= Duration::from_millis(25));
    assert!(timings.ttfb_ms < 40);
    worker.join().unwrap();
}

#[test]
fn request_delay_timeout_returns_504_and_records_trace_flags() {
    let mut state = test_state();
    state.config.request_total_timeout = Duration::from_millis(40);
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        rsproxy_rules::RuleSet::parse("default", "example.test delay(req, 100ms) status(209)")
            .unwrap(),
    );
    let request = RawRequest {
        method: "GET".to_string(),
        target: "http://example.test/delayed".to_string(),
        version: "HTTP/1.1".to_string(),
        headers: vec![("Host".to_string(), "example.test".to_string())],
        body: Vec::new(),
        trailers: Vec::new(),
    };
    let mut capture = CapturedHttpResponse::default();

    let connection = handle_http_stream(
        &mut capture,
        request,
        state.clone(),
        test_connection_input(),
    )
    .unwrap();

    assert_eq!(connection, ClientPersistence::KeepAlive);
    assert!(
        capture
            .bytes
            .starts_with(b"HTTP/1.1 504 Gateway Timeout\r\n")
    );
    assert!(
        String::from_utf8_lossy(&capture.bytes).contains("stage=request_total: timeout after 40ms")
    );
    let sessions = state.trace.list(1);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, Some(504));
    assert!(sessions[0].flags.contains(&"request-timeout".to_string()));
    assert!(
        sessions[0]
            .flags
            .contains(&"request-total-timeout".to_string())
    );
}
