use super::*;

#[test]
fn large_fixed_upload_streams_and_preserves_client_keep_alive() {
    let payload = (0..2 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    let expected_prefix = payload[..32].to_vec();
    let (origin, requests, origin_worker) = spawn_origin(2, |index, request| {
        (
            Vec::new(),
            if index == 0 {
                format!("upload={}", request.body.len()).into_bytes()
            } else {
                b"next".to_vec()
            },
        )
    });
    let mut state = test_state();
    state.config.trace_body_limit = 32;
    state.config.body_buffer_limit = 64;
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);

    write!(
        client,
        "POST http://{origin}/upload HTTP/1.1\r\nHost: {origin}\r\nContent-Length: {}\r\n\r\n",
        payload.len()
    )
    .unwrap();
    client.write_all(&payload).unwrap();
    client.flush().unwrap();
    let (first_head, first_body) = read_response(&mut client);
    assert_eq!(first_head.status, 200);
    assert_eq!(first_body.body, b"upload=2097152");
    assert_eq!(
        response_header(&first_head, "connection"),
        Some("keep-alive")
    );

    write!(
        client,
        "GET http://{origin}/next HTTP/1.1\r\nHost: {origin}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    client.flush().unwrap();
    let (_, second_body) = read_response(&mut client);
    assert_eq!(second_body.body, b"next");
    drop(client);

    proxy_worker.join().unwrap();
    origin_worker.join().unwrap();
    let uploaded = requests.recv().unwrap();
    let next = requests.recv().unwrap();
    assert_eq!(uploaded.body, payload);
    assert_eq!(next.target, "/next");

    let sessions = state.trace.list(10);
    let upload = sessions
        .iter()
        .find(|session| session.url.ends_with("/upload"))
        .unwrap();
    assert_eq!(upload.request_bytes, 2 * 1024 * 1024);
    assert_eq!(upload.req_body_head, expected_prefix);
    assert!(upload.flags.contains(&"request-streamed".to_string()));
    assert!(upload.flags.contains(&"h1-client-keepalive".to_string()));
    let next = sessions
        .iter()
        .find(|session| session.url.ends_with("/next"))
        .unwrap();
    assert!(
        next.flags
            .contains(&"h1-client-connection-reused".to_string())
    );
}

#[test]
fn streamed_request_body_is_visible_to_collector_before_upload_finishes() {
    let payload = vec![b'r'; 8 * 1024];
    let (origin, requests, origin_worker) = spawn_origin(1, |_, request| {
        (
            Vec::new(),
            format!("upload={}", request.body.len()).into_bytes(),
        )
    });
    let mut state = test_state();
    state.config.trace_body_limit = payload.len();
    state.config.body_buffer_limit = 1;
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);

    write!(
        client,
        "POST http://{origin}/pending HTTP/1.1\r\nHost: {origin}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )
    .unwrap();
    client.write_all(&payload[..4096]).unwrap();
    client.flush().unwrap();

    let pending = wait_for_trace_stats(&state.trace, |stats| {
        stats.pending_sessions == 1 && stats.pending_memory_bytes >= 4096
    });
    assert_eq!(pending.sessions, 0);
    assert!(state.trace.list(1).is_empty());

    thread::sleep(Duration::from_millis(60));
    client.write_all(&payload[4096..]).unwrap();
    client.flush().unwrap();
    let (head, body) = read_response(&mut client);
    assert_eq!(head.status, 200);
    assert_eq!(body.body, b"upload=8192");
    drop(client);

    proxy_worker.join().unwrap();
    origin_worker.join().unwrap();
    assert_eq!(requests.recv().unwrap().body, payload);
    let session = state.trace.list(1).pop().unwrap();
    assert_eq!(session.request_bytes, 8 * 1024);
    assert_eq!(session.req_body_head, vec![b'r'; 8 * 1024]);
    assert!(session.request_send_ms.unwrap() >= 50);
    assert!(session.response_receive_ms.is_some());
}

#[test]
fn expect_continue_is_answered_before_the_streamed_body_is_read() {
    let (origin, requests, origin_worker) = spawn_origin(1, |_, request| {
        let expect_forwarded = http::header(&request.headers, "expect").is_some();
        (
            Vec::new(),
            format!("body={};expect={expect_forwarded}", request.body.len()).into_bytes(),
        )
    });
    let state = test_state();
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);

    write!(
        client,
        "POST http://{origin}/expect HTTP/1.1\r\nHost: {origin}\r\nContent-Length: 5\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    client.flush().unwrap();
    let interim = http::read_response_head(&mut client, 4096, 16).unwrap();
    assert_eq!(interim.status, 100);

    client.write_all(b"hello").unwrap();
    client.flush().unwrap();
    let (head, body) = read_response(&mut client);
    assert_eq!(head.status, 200);
    assert_eq!(body.body, b"body=5;expect=false");
    drop(client);

    proxy_worker.join().unwrap();
    origin_worker.join().unwrap();
    assert_eq!(requests.recv().unwrap().body, b"hello");
    let session = state.trace.list(1).pop().unwrap();
    assert!(session.flags.contains(&"expect-continue".to_string()));
    assert_eq!(session.request_bytes, 5);
}

#[test]
fn proxy_auth_rejects_before_reading_or_acknowledging_the_body() {
    let mut state = test_state();
    state.config.proxy_auth = Some("user:pass".to_string());
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);

    client
        .write_all(
            b"POST http://127.0.0.1:9/upload HTTP/1.1\r\nHost: 127.0.0.1:9\r\nContent-Length: 1048576\r\nExpect: 100-continue\r\n\r\n",
        )
        .unwrap();
    client.flush().unwrap();
    let (head, body) = read_response(&mut client);
    assert_eq!(head.status, 407);
    assert_eq!(response_header(&head, "connection"), Some("close"));
    assert_eq!(body.body, b"proxy authentication required\n");
    drop(client);

    proxy_worker.join().unwrap();
    assert!(state.trace.list(10).is_empty());
}

#[test]
fn slow_streamed_upload_obeys_the_request_total_deadline() {
    let origin_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = origin_listener.local_addr().unwrap();
    let origin_worker = thread::spawn(move || {
        let (mut stream, _) = origin_listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        assert!(http::read_request(&mut stream, 4096, 32).is_err());
    });
    let mut state = test_state();
    state.config.body_buffer_limit = 1;
    state.config.request_total_timeout = Duration::from_millis(80);
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);

    let started = Instant::now();
    write!(
        client,
        "POST http://{origin}/slow HTTP/1.1\r\nHost: {origin}\r\nContent-Length: 8\r\nConnection: close\r\n\r\na"
    )
    .unwrap();
    client.flush().unwrap();
    let (head, _) = read_response(&mut client);
    assert_eq!(head.status, 504);
    assert!(started.elapsed() >= Duration::from_millis(60));
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(client);

    proxy_worker.join().unwrap();
    origin_worker.join().unwrap();
    let session = state.trace.list(1).pop().unwrap();
    assert!(
        session
            .error
            .as_deref()
            .unwrap_or("")
            .starts_with("stage=request_total: timeout after 80ms")
    );
    assert!(session.flags.contains(&"request-timeout".to_string()));
    assert!(session.flags.contains(&"request-total-timeout".to_string()));
}

#[test]
fn response_head_timeout_preserves_the_completed_request_send_boundary() {
    let origin_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = origin_listener.local_addr().unwrap();
    let origin_worker = thread::spawn(move || {
        let (mut stream, _) = origin_listener.accept().unwrap();
        let request = http::read_request(&mut stream, 4096, 32).unwrap().unwrap();
        assert_eq!(request.body, b"complete");
        thread::sleep(Duration::from_millis(100));
    });
    let mut state = test_state();
    state.config.body_buffer_limit = 1;
    state.config.upstream_ttfb_timeout = Duration::from_millis(40);
    state.config.request_total_timeout = Duration::from_secs(1);
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);

    write!(
        client,
        "POST http://{origin}/silent HTTP/1.1\r\nHost: {origin}\r\nContent-Length: 8\r\nConnection: close\r\n\r\ncomplete"
    )
    .unwrap();
    client.flush().unwrap();
    let (head, _) = read_response(&mut client);
    assert_eq!(head.status, 504);
    drop(client);

    proxy_worker.join().unwrap();
    origin_worker.join().unwrap();
    let session = state.trace.list(1).pop().unwrap();
    assert!(session.flags.contains(&"upstream-ttfb-timeout".to_string()));
    assert!(session.request_send_ms.is_some());
    assert_eq!(session.response_receive_ms, None);
}
