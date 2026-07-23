use super::*;
use rsproxy_rules::MatchedRule;

#[test]
fn spill_json_covers_all_session_frame_tls_and_optional_shapes() {
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
            "GET".to_string(),
            "http://example.test/\"quoted\"".to_string(),
            "127.0.0.1:1".to_string(),
        );
        session.id = index as u64 + 1;
        session.status = (index == 0).then_some(200);
        session.request_send_ms = (index == 0).then_some(2);
        session.response_receive_ms = (index == 0).then_some(3);
        session.upstream = (index == 0).then(|| "example.test:80".to_string());
        session.error = (index == 1).then(|| "closed\nby\tpeer\u{0001}".to_string());
        session.flags = vec!["one".to_string(), "two".to_string()];
        session.matched_rules.push(MatchedRule {
            group: "default".into(),
            line: 1,
            raw: "example.test upstream(socks5://user:secret@proxy.test:1080)".into(),
        });
        session.req_headers = vec![("X-Test".to_string(), "yes".to_string())];
        session.req_trailers = vec![("X-Request-End".to_string(), "yes".to_string())];
        session.req_body_head = vec![b'a', 0xff];
        session.res_headers = vec![("Content-Type".to_string(), "text/plain".to_string())];
        session.res_trailers = vec![("X-Response-End".to_string(), "yes".to_string())];
        session.res_body_head = b"body".to_vec();
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
        session.tls = vec![
            TlsRecord {
                phase: "origin".to_string(),
                host: "example.test".to_string(),
                handshake_ms: 4,
                peer_certificates: 1,
                protocol: Some("TLSv1.3".to_string()),
                cipher_suite: Some("TLS_AES_128_GCM_SHA256".to_string()),
                alpn: Some("h2".to_string()),
                error: None,
            },
            TlsRecord {
                phase: "failed".to_string(),
                host: "bad.test".to_string(),
                handshake_ms: 0,
                peer_certificates: 0,
                protocol: None,
                cipher_suite: None,
                alpn: None,
                error: Some("alert".to_string()),
            },
        ];

        let line = serialize::spill_session_line(&session);
        let value: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(value["id"], session.id);
        assert_eq!(value["kind"], ["http", "tunnel", "sse", "websocket"][index]);
        assert_eq!(value["frames"][0]["direction"], "c2s");
        assert_eq!(value["frames"][0]["data"], "hello");
        assert_eq!(value["frames"][1]["direction"], "s2c");
        assert_eq!(value["frames"][1]["data"], "00ab");
        assert_eq!(value["frames"][1]["truncated"], true);
        assert_eq!(value["tls"][0]["protocol"], "TLSv1.3");
        assert_eq!(value["tls"][1]["protocol"], serde_json::Value::Null);
        assert_eq!(value["tls"][1]["error"], "alert");
        assert!(
            value["rules"][0]["raw"]
                .as_str()
                .unwrap()
                .contains("socks5://auth@")
        );
    }
}

#[test]
fn fallback_stats_and_debug_output_preserve_configured_resource_limits() {
    let store = TraceStore::new_with_limits(9, 7, 4096, None);
    let stats = store.empty_stats();
    assert_eq!(stats.sessions, 0);
    assert_eq!(stats.max_sessions, 9);
    assert_eq!(stats.queue_capacity, 7);
    assert_eq!(stats.memory_budget_bytes, 4096);
    assert_eq!(stats.total_memory_bytes, stats.queue_bytes);
    assert_eq!(stats.spill_errors, 1);
    assert_eq!(
        stats.last_spill_error.as_deref(),
        Some("trace collector is unavailable")
    );
    let debug = format!("{store:?}");
    assert!(debug.contains("max_sessions: 9"));
    assert!(debug.contains("queue_capacity: 7"));
    assert!(debug.contains("memory_budget_bytes: 4096"));
}
