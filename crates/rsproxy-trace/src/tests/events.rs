use super::*;
use bytes::Bytes;

#[test]
fn incremental_events_assemble_one_complete_session_with_bounded_body_previews() {
    let store = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 8,
        queue_capacity: 32,
        memory_budget_bytes: 1024 * 1024,
        queue_memory_budget_bytes: None,
        body_limit: 4,
        spill: None,
    });
    let id = store.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 123,
        method: "POST".to_string(),
        url: "http://example.test/original".to_string(),
        client: "127.0.0.1:12345".to_string(),
    });
    assert!(store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Request,
        data: Bytes::from_static(b"abc"),
        observed_bytes: 5,
    }));
    assert!(store.emit(TraceEvent::Request {
        id,
        method: Some("PUT".to_string()),
        url: Some("http://example.test/rewritten".to_string()),
        headers: vec![("content-type".to_string(), "text/plain".to_string())],
        trailers: vec![("x-request-end".to_string(), "yes".to_string())],
        matched_rules: Vec::new(),
    }));
    assert!(store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Request,
        data: Bytes::from_static(b"def"),
        observed_bytes: 4,
    }));
    assert!(store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Response,
        data: Bytes::from_static(b"response"),
        observed_bytes: 8,
    }));
    assert!(store.emit(TraceEvent::Response {
        id,
        status: Some(201),
        headers: vec![("content-type".to_string(), "text/plain".to_string())],
        trailers: vec![("x-response-end".to_string(), "yes".to_string())],
    }));
    assert!(store.emit(TraceEvent::Frame {
        id,
        frame: FrameRecord::new(
            FrameDirection::ServerToClient,
            130,
            "text",
            true,
            b"frame",
            8,
            FrameDataEncoding::Utf8,
        ),
    }));
    assert!(store.emit(TraceEvent::Tls {
        id,
        record: TlsRecord {
            phase: "upstream_tls".to_string(),
            host: "example.test".to_string(),
            handshake_ms: 7,
            peer_certificates: 1,
            protocol: Some("TLSv1_3".to_string()),
            cipher_suite: None,
            alpn: Some("h2".to_string()),
            error: None,
        },
    }));
    assert!(store.emit(TraceEvent::End {
        id,
        kind: SessionKind::Http,
        duration_ms: 20,
        pool_wait_ms: 1,
        dns_ms: 2,
        connect_ms: 3,
        ttfb_ms: 4,
        request_send_ms: Some(5),
        response_receive_ms: Some(6),
        upstream: Some("example.test:443".to_string()),
        flags: vec!["h2-upstream".to_string()],
        error: None,
    }));

    let session = store.get(id).unwrap();
    assert_eq!(session.started_ms, 123);
    assert_eq!(session.method, "PUT");
    assert_eq!(session.url, "http://example.test/rewritten");
    assert_eq!(session.status, Some(201));
    assert_eq!(session.request_bytes, 9);
    assert_eq!(session.response_bytes, 8);
    assert_eq!(session.req_body_head, b"abcd");
    assert_eq!(session.res_body_head, b"resp");
    assert_eq!(session.req_trailers[0].0, "x-request-end");
    assert_eq!(session.res_trailers[0].0, "x-response-end");
    assert_eq!(session.frames.len(), 1);
    assert_eq!(session.tls[0].alpn.as_deref(), Some("h2"));
    assert_eq!(session.upstream.as_deref(), Some("example.test:443"));
    assert_eq!(session.duration_ms, 20);
    assert_eq!(session.request_send_ms, Some(5));
    assert_eq!(session.response_receive_ms, Some(6));
    assert_eq!(store.stats().pending_sessions, 0);
}

#[test]
fn abort_and_unknown_events_have_explicit_partial_session_stats() {
    let store = TraceStore::new_with_limits(8, 8, 1024 * 1024, None);
    let id = store.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 1,
        method: "GET".to_string(),
        url: "http://example.test/".to_string(),
        client: "127.0.0.1:1".to_string(),
    });
    assert_eq!(store.stats().pending_sessions, 1);

    assert!(store.abort(id));
    assert!(store.emit(TraceEvent::BodyChunk {
        id: id + 100,
        direction: BodyDirection::Response,
        data: Bytes::new(),
        observed_bytes: 1,
    }));
    let stats = store.stats();

    assert_eq!(stats.pending_sessions, 0);
    assert_eq!(stats.incomplete_sessions, 1);
    assert_eq!(stats.orphan_events, 1);
}

#[test]
fn growing_partial_sessions_evict_completed_data_then_abort_at_the_budget() {
    let probe = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 8,
        queue_capacity: 8,
        memory_budget_bytes: 1024 * 1024,
        queue_memory_budget_bytes: None,
        body_limit: 16 * 1024,
        spill: None,
    });
    let probe_id = probe.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 1,
        method: "POST".to_string(),
        url: "http://example.test/".to_string(),
        client: "127.0.0.1:1".to_string(),
    });
    let base_bytes = probe.stats().pending_memory_bytes;
    probe.abort(probe_id);
    drop(probe);

    let store = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 8,
        queue_capacity: 8,
        memory_budget_bytes: base_bytes + 4096 + 8192,
        queue_memory_budget_bytes: Some(8192),
        body_limit: 16 * 1024,
        spill: None,
    });
    let id = store.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 1,
        method: "POST".to_string(),
        url: "http://example.test/".to_string(),
        client: "127.0.0.1:1".to_string(),
    });
    assert!(store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Request,
        data: Bytes::from(vec![b'a'; 3000]),
        observed_bytes: 3000,
    }));
    let within_budget = store.stats();
    assert_eq!(within_budget.pending_sessions, 1);
    assert!(within_budget.memory_bytes <= within_budget.memory_budget_bytes);

    assert!(store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Request,
        data: Bytes::from(vec![b'b'; 3000]),
        observed_bytes: 3000,
    }));
    let over_budget = store.stats();
    assert_eq!(over_budget.pending_sessions, 0);
    assert_eq!(over_budget.incomplete_sessions, 1);
    assert_eq!(over_budget.queue_memory_dropped, 0);
    assert!(over_budget.memory_bytes <= over_budget.memory_budget_bytes);
}

#[test]
fn final_snapshot_corrects_body_chunks_dropped_by_a_full_queue() {
    let store = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 8,
        queue_capacity: 2,
        memory_budget_bytes: 1024 * 1024,
        queue_memory_budget_bytes: None,
        body_limit: 16,
        spill: None,
    });
    let blocked = store.block_collector();
    let id = store.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 1,
        method: "POST".to_string(),
        url: "http://example.test/upload".to_string(),
        client: "127.0.0.1:1".to_string(),
    });
    assert!(store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Request,
        data: Bytes::from_static(b"abc"),
        observed_bytes: 3,
    }));
    assert!(!store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Request,
        data: Bytes::from_static(b"def"),
        observed_bytes: 3,
    }));
    drop(blocked);
    assert_eq!(store.stats().pending_sessions, 1);

    let mut final_session = Session::new(
        SessionKind::Http,
        "POST".to_string(),
        "http://example.test/upload".to_string(),
        "127.0.0.1:1".to_string(),
    );
    final_session.id = id;
    final_session.request_bytes = 6;
    final_session.req_body_head = b"abcdef".to_vec();
    assert!(store.finish(final_session));

    let session = store.get(id).unwrap();
    assert_eq!(session.request_bytes, 6);
    assert_eq!(session.req_body_head, b"abcdef");
    let stats = store.stats();
    assert_eq!(stats.queue_dropped, 1);
    assert_eq!(stats.pending_sessions, 0);
    assert_eq!(stats.orphan_events, 0);
}

#[test]
fn concurrent_body_event_producers_preserve_counts_for_both_directions() {
    let store = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 8,
        queue_capacity: 2048,
        memory_budget_bytes: 4 * 1024 * 1024,
        queue_memory_budget_bytes: None,
        body_limit: 16,
        spill: None,
    });
    let id = store.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 1,
        method: "POST".to_string(),
        url: "http://example.test/duplex".to_string(),
        client: "127.0.0.1:1".to_string(),
    });
    assert_eq!(store.stats().pending_sessions, 1);

    let mut workers = Vec::new();
    for worker in 0..8 {
        let store = store.clone();
        workers.push(std::thread::spawn(move || {
            let direction = if worker % 2 == 0 {
                BodyDirection::Request
            } else {
                BodyDirection::Response
            };
            for _ in 0..100 {
                assert!(store.emit(TraceEvent::BodyChunk {
                    id,
                    direction,
                    data: Bytes::from_static(b"x"),
                    observed_bytes: 1,
                }));
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(store.stats().queue_dropped, 0);
    assert!(store.emit(TraceEvent::End {
        id,
        kind: SessionKind::Http,
        duration_ms: 1,
        pool_wait_ms: 0,
        dns_ms: 0,
        connect_ms: 0,
        ttfb_ms: 0,
        request_send_ms: None,
        response_receive_ms: None,
        upstream: None,
        flags: Vec::new(),
        error: None,
    }));

    let session = store.get(id).unwrap();
    assert_eq!(session.request_bytes, 400);
    assert_eq!(session.response_bytes, 400);
    assert_eq!(session.req_body_head, vec![b'x'; 16]);
    assert_eq!(session.res_body_head, vec![b'x'; 16]);
}

#[test]
fn body_event_queue_budget_uses_observed_chunk_size_after_preview_limit() {
    let store = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 8,
        queue_capacity: 8,
        memory_budget_bytes: 2048,
        queue_memory_budget_bytes: Some(512),
        body_limit: 0,
        spill: None,
    });
    let id = store.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 1,
        method: "GET".to_string(),
        url: "http://example.test/".to_string(),
        client: String::new(),
    });
    assert_eq!(store.stats().pending_sessions, 1);

    assert!(!store.emit(TraceEvent::BodyChunk {
        id,
        direction: BodyDirection::Response,
        data: Bytes::new(),
        observed_bytes: 1024,
    }));
    let stats = store.stats();
    assert_eq!(stats.queue_memory_dropped, 1);
    assert_eq!(stats.pending_sessions, 1);
    store.abort(id);
}
