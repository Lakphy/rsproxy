use super::*;

#[test]
fn tunnel_trace_events_count_both_directions_without_capturing_payloads() {
    let store = rsproxy_trace::TraceStore::new(4);
    let id = store.start(rsproxy_trace::SessionStart {
        kind: SessionKind::Tunnel,
        started_ms: rsproxy_trace::now_millis(),
        method: "CONNECT".to_string(),
        url: "example.test:443".to_string(),
        client: "127.0.0.1:1".to_string(),
    });
    let trace = TunnelTrace::new(store.clone(), id).unwrap();

    trace.observe(rsproxy_trace::BodyDirection::Request, 0);
    trace.observe(rsproxy_trace::BodyDirection::Request, 11);
    trace.observe(rsproxy_trace::BodyDirection::Response, 7);
    store.emit(rsproxy_trace::TraceEvent::End {
        id,
        kind: SessionKind::Tunnel,
        duration_ms: 1,
        pool_wait_ms: 0,
        dns_ms: 0,
        connect_ms: 0,
        ttfb_ms: 0,
        request_send_ms: None,
        response_receive_ms: None,
        upstream: Some("example.test:443".to_string()),
        flags: vec!["tunnel".to_string()],
        error: None,
    });

    let session = store.list(1).pop().unwrap();
    assert_eq!(session.request_bytes, 11);
    assert_eq!(session.response_bytes, 7);
    assert!(session.req_body_head.is_empty());
    assert!(session.res_body_head.is_empty());
    assert!(TunnelTrace::new(store, 0).is_none());
}
