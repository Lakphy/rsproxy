use super::*;

#[test]
fn throttle_pacer_carries_the_rate_limit_across_separate_writes() {
    let mut output = Vec::new();
    let mut pacer = ThrottlePacer::new(Some(1024 * 1024));
    pacer.write(&mut output, &[b'a'; 16 * 1024]).unwrap();

    let started = Instant::now();
    pacer.write(&mut output, b"b").unwrap();

    assert!(started.elapsed() >= Duration::from_millis(8));
    assert_eq!(output.len(), 16 * 1024 + 1);
}

#[test]
fn throttle_pacer_obeys_the_absolute_request_deadline() {
    let mut output = Vec::new();
    let mut pacer = ThrottlePacer::new(Some(1));
    pacer
        .write_until(
            &mut output,
            b"a",
            RequestDeadline::new(Duration::from_millis(20)).unwrap(),
        )
        .unwrap();
    let deadline = RequestDeadline::new(Duration::from_millis(20)).unwrap();

    let error = pacer.write_until(&mut output, b"b", deadline).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(output, b"a");
}

#[test]
fn throttle_pacer_normalizes_programmatic_zero_rates() {
    let pacer = ThrottlePacer::new(Some(0));

    assert_eq!(pacer.bytes_per_sec, Some(1));
}

#[test]
fn websocket_urls_have_distinct_rule_and_transport_schemes() {
    let request = RawRequest {
        method: "GET".to_string(),
        target: "ws://socket.test/live".to_string(),
        version: "HTTP/1.1".to_string(),
        headers: vec![
            ("Host".to_string(), "socket.test".to_string()),
            ("Upgrade".to_string(), "websocket".to_string()),
            ("Connection".to_string(), "Upgrade".to_string()),
        ],
        body: Vec::new(),
        trailers: Vec::new(),
    };

    let transport = absolute_url_for(&request, None).unwrap();
    assert_eq!(transport, "http://socket.test/live");
    assert_eq!(rule_url_for(&transport, &request.headers), request.target);

    let secure_transport = absolute_url_for(
        &RawRequest {
            target: "/live".to_string(),
            ..request.clone()
        },
        Some("socket.test"),
    )
    .unwrap();
    assert_eq!(secure_transport, "https://socket.test/live");
    assert_eq!(
        rule_url_for(&secure_transport, &request.headers),
        "wss://socket.test/live"
    );
}
