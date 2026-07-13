use super::*;

#[test]
fn body_rules_apply_below_limit_and_skip_only_body_behavior_above_it() {
    let (origin, requests, origin_worker) = spawn_origin(2, |_, request| {
        (
            Vec::new(),
            format!(
                "body={};kept={}",
                String::from_utf8_lossy(&request.body),
                http::header(&request.headers, "x-kept").unwrap_or("missing")
            )
            .into_bytes(),
        )
    });
    let mut state = test_state();
    state.config.body_buffer_limit = 4;
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        RuleSet::parse(
            "default",
            "127.0.0.1 req.header(x-kept: yes) req.body.append(\"!\")\n127.0.0.1 res.header(x-body-match: yes) when body(~abc)\n127.0.0.1 res.header(x-status: yes) when status(200)",
        )
        .unwrap(),
    );
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 2);

    let mut small = connect_client(proxy);
    write!(
        small,
        "POST http://{origin}/small HTTP/1.1\r\nHost: {origin}\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc"
    )
    .unwrap();
    small.flush().unwrap();
    let (small_head, small_body) = read_response(&mut small);
    assert_eq!(small_body.body, b"body=abc!;kept=yes");
    assert_eq!(response_header(&small_head, "x-body-match"), Some("yes"));
    assert_eq!(response_header(&small_head, "x-status"), Some("yes"));
    drop(small);

    let mut large = connect_client(proxy);
    write!(
        large,
        "POST http://{origin}/large HTTP/1.1\r\nHost: {origin}\r\nContent-Length: 8\r\nConnection: close\r\n\r\nabcdefgh"
    )
    .unwrap();
    large.flush().unwrap();
    let (large_head, large_body) = read_response(&mut large);
    assert_eq!(large_body.body, b"body=abcdefgh;kept=yes");
    assert_eq!(response_header(&large_head, "x-body-match"), None);
    assert_eq!(response_header(&large_head, "x-status"), Some("yes"));
    drop(large);

    proxy_worker.join().unwrap();
    origin_worker.join().unwrap();
    assert_eq!(requests.recv().unwrap().body, b"abc!");
    assert_eq!(requests.recv().unwrap().body, b"abcdefgh");

    let sessions = state.trace.list(10);
    let small = sessions
        .iter()
        .find(|session| session.url.ends_with("/small"))
        .unwrap();
    assert!(!small.flags.contains(&"request-streamed".to_string()));
    let large = sessions
        .iter()
        .find(|session| session.url.ends_with("/large"))
        .unwrap();
    assert!(large.flags.contains(&"request-streamed".to_string()));
    assert!(
        large
            .flags
            .contains(&"request-body-rewrite-skipped-limit".to_string())
    );
}
