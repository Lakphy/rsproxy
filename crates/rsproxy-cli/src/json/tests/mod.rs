use super::*;
use rsproxy_rules::MatchedRule;
use serde_json::Value as JsonValue;

#[test]
fn trace_stats_exposes_queue_and_memory_limits() {
    let store = rsproxy_trace::TraceStore::new_with_limits(4, 7, 4096, None);

    let value: JsonValue = serde_json::from_str(&stats(store.stats())).unwrap();

    assert_eq!(value["queue_capacity"], 7);
    assert_eq!(value["queue_dropped"], 0);
    assert_eq!(value["queue_bytes"], 0);
    assert_eq!(value["queue_memory_budget_bytes"], 1024);
    assert_eq!(value["queue_memory_dropped"], 0);
    assert_eq!(value["memory_budget_bytes"], 4096);
    assert_eq!(value["memory_bytes"], 0);
    assert_eq!(value["completed_memory_bytes"], 0);
    assert_eq!(value["pending_memory_bytes"], 0);
    assert_eq!(value["resident_memory_budget_bytes"], 3072);
    assert_eq!(value["total_memory_bytes"], 0);
    assert_eq!(value["evicted_sessions"], 0);
    assert_eq!(value["pending_sessions"], 0);
    assert_eq!(value["incomplete_sessions"], 0);
    assert_eq!(value["orphan_events"], 0);
    assert_eq!(value["follow_subscribers"], 0);
    assert_eq!(value["follow_dropped"], 0);
}

#[test]
fn har_is_rfc3339_and_preserves_rsproxy_diagnostics() {
    let mut session = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "https://example.test/items?q=1".to_string(),
        "127.0.0.1:12345".to_string(),
    );
    session.id = 42;
    session.started_ms = 1_700_000_000_123;
    session.duration_ms = 100;
    session.pool_wait_ms = 10;
    session.dns_ms = 5;
    session.connect_ms = 7;
    session.ttfb_ms = 20;
    session.request_send_ms = Some(3);
    session.response_receive_ms = Some(17);
    session.status = Some(504);
    session.upstream = Some("example.test:443".to_string());
    session.request_bytes = 3;
    session.response_bytes = 4;
    session.flags = vec!["h2-client".to_string(), "request-total-timeout".to_string()];
    session.error = Some("stage=request_total: \"quoted\"".to_string());
    session.matched_rules.push(MatchedRule {
        group: "default".to_string(),
        line: 3,
        raw: "example.test tag(exported)".to_string(),
    });
    session
        .res_headers
        .push(("content-type".to_string(), "text/plain".to_string()));
    session.res_body_head = b"fail".to_vec();
    session.tls.push(tls_record("client_mitm_tls", 13));
    session.tls.push(tls_record("upstream_tls", 11));

    let har: JsonValue = serde_json::from_str(&sessions_har(&[session])).unwrap();
    let entry = &har["log"]["entries"][0];

    assert_eq!(entry["startedDateTime"], "2023-11-14T22:13:20.123Z");
    assert_eq!(entry["request"]["httpVersion"], "HTTP/2");
    assert_eq!(entry["request"]["queryString"][0]["name"], "q");
    assert_eq!(entry["request"]["queryString"][0]["value"], "1");
    assert_eq!(entry["response"]["httpVersion"], "HTTP/2");
    assert_eq!(entry["timings"]["ssl"], 11);
    assert_eq!(entry["timings"]["blocked"], 37);
    assert_eq!(entry["timings"]["send"], 3);
    assert_eq!(entry["timings"]["receive"], 17);
    assert_eq!(entry["_rsproxy"]["session_id"], 42);
    assert_eq!(
        entry["_rsproxy"]["error"],
        "stage=request_total: \"quoted\""
    );
    assert_eq!(entry["_rsproxy"]["flags"][1], "request-total-timeout");
    assert_eq!(entry["_rsproxy"]["rules"][0]["line"], 3);
    assert_eq!(entry["_rsproxy"]["tls"][1]["phase"], "upstream_tls");
    assert_eq!(entry["_rsproxy"]["timings"]["client_tls_ms"], 13);
    assert_eq!(
        entry["_rsproxy"]["timings"]["client_tls_in_timeline"],
        false
    );
    assert_eq!(entry["_rsproxy"]["timings"]["upstream_tls_ms"], 11);
    assert_eq!(entry["_rsproxy"]["timings"]["recorded_tls_ms"], 24);
    assert_eq!(entry["_rsproxy"]["timings"]["timeline_tls_ms"], 11);
    assert_eq!(entry["_rsproxy"]["timings"]["request_send_ms"], 3);
    assert_eq!(entry["_rsproxy"]["timings"]["response_receive_ms"], 17);
    assert_eq!(entry["_rsproxy"]["timings"]["boundaries_complete"], true);
    assert_eq!(entry["_rsproxy"]["timings"]["unattributed_ms"], 27);

    let timings = &entry["timings"];
    let total = [
        "blocked", "dns", "connect", "ssl", "send", "wait", "receive",
    ]
    .iter()
    .map(|name| timings[name].as_u64().unwrap())
    .sum::<u64>();
    assert_eq!(total, entry["time"].as_u64().unwrap());
}

#[test]
fn har_marks_absent_ssl_and_excludes_tunnels() {
    let mut http = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "http://example.test/".to_string(),
        "127.0.0.1:1".to_string(),
    );
    http.duration_ms = 9;
    http.status = Some(200);
    http.tls.push(tls_record("client_mitm_tls", 5));
    let tunnel = Session::new(
        SessionKind::Tunnel,
        "CONNECT".to_string(),
        "example.test:443".to_string(),
        "127.0.0.1:2".to_string(),
    );

    let har: JsonValue = serde_json::from_str(&sessions_har(&[http, tunnel])).unwrap();
    let entries = har["log"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["timings"]["ssl"], -1);
    assert_eq!(entries[0]["timings"]["receive"], 9);
    assert_eq!(
        entries[0]["_rsproxy"]["timings"]["client_tls_in_timeline"],
        true
    );
    assert_eq!(entries[0]["_rsproxy"]["timings"]["timeline_tls_ms"], 5);
    assert_eq!(entries[0]["_rsproxy"]["timings"]["unattributed_ms"], 4);
}

#[test]
fn har_projects_duplex_transfer_overlap_without_losing_exact_diagnostics() {
    let mut session = Session::new(
        SessionKind::Http,
        "POST".to_string(),
        "https://example.test/duplex".to_string(),
        "127.0.0.1:1".to_string(),
    );
    session.duration_ms = 100;
    session.request_send_ms = Some(80);
    session.response_receive_ms = Some(70);
    session.status = Some(200);

    let har: JsonValue = serde_json::from_str(&sessions_har(&[session])).unwrap();
    let entry = &har["log"]["entries"][0];

    assert_eq!(entry["timings"]["send"], 80);
    assert_eq!(entry["timings"]["receive"], 20);
    assert_eq!(entry["_rsproxy"]["timings"]["response_receive_ms"], 70);
    assert_eq!(entry["_rsproxy"]["timings"]["transfer_overlap_ms"], 50);
    let timings = &entry["timings"];
    let total = [
        "blocked", "dns", "connect", "ssl", "send", "wait", "receive",
    ]
    .iter()
    .map(|name| {
        let value = timings[name].as_i64().unwrap();
        value.max(0) as u64
    })
    .sum::<u64>();
    assert_eq!(total, entry["time"].as_u64().unwrap());
}

#[test]
fn session_json_covers_kinds_frames_tls_headers_and_secret_redaction() {
    let mut sessions = Vec::new();
    for (index, kind) in [
        SessionKind::Http,
        SessionKind::Tunnel,
        SessionKind::Sse,
        SessionKind::WebSocket,
    ]
    .into_iter()
    .enumerate()
    {
        let mut session = Session::new(
            kind,
            if kind == SessionKind::Tunnel {
                "CONNECT".to_string()
            } else {
                "GET".to_string()
            },
            format!("http://example.test/{}", "x".repeat(100)),
            "127.0.0.1:1".to_string(),
        );
        session.id = index as u64 + 1;
        session.status = (index != 1).then_some(200);
        session.request_send_ms = (index == 0).then_some(2);
        session.response_receive_ms = (index == 0).then_some(3);
        session.upstream = (index == 0).then(|| "example.test:80".to_string());
        session.error = (index == 1).then(|| "closed\nby peer".to_string());
        session.flags = vec!["one".to_string(), "two".to_string()];
        session.matched_rules.push(MatchedRule {
            group: "secret".to_string(),
            line: 9,
            raw: "example.test upstream(socks5://user:password@proxy.test:1080)".to_string(),
        });
        session.req_headers = vec![("X-Quote".to_string(), "\"value\"".to_string())];
        session.req_trailers = vec![("X-Request-End".to_string(), "yes".to_string())];
        session.req_body_head = vec![b'a', 0xff];
        session.res_headers = vec![("Content-Type".to_string(), "text/plain".to_string())];
        session.res_trailers = vec![("X-Response-End".to_string(), "yes".to_string())];
        session.res_body_head = b"response".to_vec();
        session.frames = vec![
            FrameRecord::new(
                FrameDirection::ClientToServer,
                1,
                "text",
                true,
                b"hello",
                5,
                FrameDataEncoding::Utf8,
            ),
            FrameRecord::new(
                FrameDirection::ServerToClient,
                2,
                "binary",
                false,
                &[0x00, 0xab, 0xff],
                2,
                FrameDataEncoding::Hex,
            ),
        ];
        session.tls = vec![TlsRecord {
            phase: "origin".to_string(),
            host: "example.test".to_string(),
            handshake_ms: 4,
            peer_certificates: 0,
            protocol: None,
            cipher_suite: None,
            alpn: None,
            error: Some("alert".to_string()),
        }];
        sessions.push(session);
    }

    let document: JsonValue = serde_json::from_str(&sessions_json(&sessions)).unwrap();
    assert_eq!(document[0]["kind"], "http");
    assert_eq!(document[1]["kind"], "tunnel");
    assert_eq!(document[2]["kind"], "sse");
    assert_eq!(document[3]["kind"], "websocket");
    assert_eq!(document[0]["frames"][0]["direction"], "c2s");
    assert_eq!(document[0]["frames"][0]["data"], "hello");
    assert_eq!(document[0]["frames"][1]["direction"], "s2c");
    assert_eq!(document[0]["frames"][1]["data"], "00ab");
    assert_eq!(document[0]["frames"][1]["truncated"], true);
    assert_eq!(document[0]["tls"][0]["error"], "alert");
    assert!(
        document[0]["rules"][0]["raw"]
            .as_str()
            .unwrap()
            .contains("socks5://auth@")
    );
    assert_eq!(document[1]["status"], JsonValue::Null);
    assert_eq!(document[1]["upstream"], JsonValue::Null);

    let summary: JsonValue = serde_json::from_str(&session_summary(&sessions[0])).unwrap();
    assert_eq!(summary["request_send_ms"], 2);
    assert_eq!(summary["response_receive_ms"], 3);
    let table = sessions_table(&sessions);
    assert!(table.starts_with("ID    KIND"));
    assert!(table.contains("..."));
}

#[test]
fn primitive_json_helpers_escape_controls_and_preserve_optional_shapes() {
    assert_eq!(escape("\"\\\n\r\t\u{0001}"), "\\\"\\\\\\n\\r\\t\\u0001");
    assert_eq!(string("value"), "\"value\"");
    assert_eq!(
        headers(&[("A".to_string(), "B".to_string())]),
        "[[\"A\",\"B\"]]"
    );
    assert_eq!(opt_string(Some("x")), "\"x\"");
    assert_eq!(opt_string(None), "null");
}

fn tls_record(phase: &str, handshake_ms: u64) -> TlsRecord {
    TlsRecord {
        phase: phase.to_string(),
        host: "example.test".to_string(),
        handshake_ms,
        peer_certificates: 1,
        protocol: Some("TLSv1_3".to_string()),
        cipher_suite: Some("TLS_AES_128_GCM_SHA256".to_string()),
        alpn: Some("h2".to_string()),
        error: None,
    }
}
