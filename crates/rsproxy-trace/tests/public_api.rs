//! Behavioral smoke tests for the tracing facade.
#![allow(clippy::unwrap_used)]

use rsproxy_trace::{Session, SessionKind, SessionStart, TraceEvent, TraceStore};
use std::time::Duration;

#[test]
fn public_trace_api_records_and_reads_a_session() {
    let store = TraceStore::new(2);
    let mut session = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "http://example.test/health".to_string(),
        "127.0.0.1:12345".to_string(),
    );
    session.status = Some(200);

    let id = store.record(session);
    let stored = store.get(id).expect("recorded session should exist");

    assert_eq!(stored.status, Some(200));
    assert_eq!(stored.url, "http://example.test/health");
    assert_eq!(store.stats().sessions, 1);
}

#[test]
fn public_follow_api_delivers_backlog_and_live_sessions() {
    let store = TraceStore::new(2);
    store.record(Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "http://example.test/one".to_string(),
        "127.0.0.1:12345".to_string(),
    ));
    let mut follow = store.follow(0, 2, 2).unwrap();
    store.record(Session::new(
        SessionKind::Http,
        "GET".to_string(),
        "http://example.test/two".to_string(),
        "127.0.0.1:12345".to_string(),
    ));

    assert_eq!(follow.try_recv().unwrap().url, "http://example.test/one");
    assert_eq!(
        follow.recv_timeout(Duration::from_millis(100)).unwrap().url,
        "http://example.test/two"
    );
}

#[test]
fn public_event_api_assembles_incremental_sessions() {
    let store = TraceStore::new(2);
    let id = store.start(SessionStart {
        kind: SessionKind::Http,
        started_ms: 10,
        method: "GET".to_string(),
        url: "http://example.test/events".to_string(),
        client: "127.0.0.1:12345".to_string(),
    });
    assert!(store.emit(TraceEvent::Response {
        id,
        status: Some(204),
        headers: Vec::new(),
        trailers: Vec::new(),
    }));
    assert!(store.emit(TraceEvent::End {
        id,
        kind: SessionKind::Sse,
        duration_ms: 3,
        pool_wait_ms: 0,
        dns_ms: 0,
        connect_ms: 0,
        ttfb_ms: 1,
        request_send_ms: Some(0),
        response_receive_ms: Some(2),
        upstream: None,
        flags: Vec::new(),
        error: None,
    }));

    let session = store.get(id).unwrap();
    assert_eq!(session.kind, SessionKind::Sse);
    assert_eq!(session.status, Some(204));
    assert_eq!(session.duration_ms, 3);
    assert_eq!(session.request_send_ms, Some(0));
    assert_eq!(session.response_receive_ms, Some(2));
}
