use super::*;

#[test]
fn plain_snapshot_renders_status_sessions_and_detail_tabs() {
    let snapshot = TuiSnapshot {
        status: serde_json::json!({
            "status": "running",
            "proxy": "127.0.0.1:8899",
            "api": "127.0.0.1:8900",
            "storage": "/tmp/rsproxy-tui",
            "trace": {
                "sessions": 2,
                "spilled": 2,
                "dropped": 0,
                "spill_compression": "zstd",
                "spill_errors": 0
            }
        }),
        sessions: vec![serde_json::json!({
            "id": 2,
            "kind": "http",
            "status": 200,
            "duration_ms": 7,
            "response_bytes": 42,
            "method": "GET",
            "url": "http://example.test/path"
        })],
        selected_detail: Some(serde_json::json!({
            "id": 2,
            "upstream": "example.test:80",
            "duration_ms": 7,
            "pool_wait_ms": 3,
            "flags": ["cache"],
            "error": null,
            "req_headers": [["Host", "example.test"]],
            "req_trailers": [["X-Request-End", "yes"]],
            "res_headers": [["X-Test", "yes"]],
            "res_trailers": [["X-Response-End", "yes"]],
            "req_body_head": "",
            "res_body_head": "body preview",
            "rules": [{"group": "default", "line": 1, "raw": "example.test cache(5)"}]
        })),
        error: None,
    };

    let headers = plain_snapshot(&snapshot, DetailTab::Headers, "example", Some("ok"));
    assert!(headers.contains("RSPROXY TUI SNAPSHOT"));
    assert!(headers.contains("spill_compression=zstd"));
    assert!(headers.contains("filter=example tab=headers"));
    assert!(headers.contains("replay=ok"));
    assert!(headers.contains("selected id=2 upstream=example.test:80 flags=cache"));
    assert!(headers.contains("Host: example.test"));
    assert!(headers.contains("X-Request-End: yes"));
    assert!(headers.contains("X-Test: yes"));
    assert!(headers.contains("X-Response-End: yes"));

    let body = plain_snapshot(&snapshot, DetailTab::Body, "", None);
    assert!(body.contains("Response body preview"));
    assert!(body.contains("body preview"));

    let rules = plain_snapshot(&snapshot, DetailTab::Rules, "", None);
    assert!(rules.contains("default:1 example.test cache(5)"));

    let overview = plain_snapshot(&snapshot, DetailTab::Overview, "", None);
    assert!(overview.contains("timing: total=7ms pool_wait=3ms"));
}

#[test]
fn session_filter_checks_summary_fields() {
    let session = serde_json::json!({
        "id": 7,
        "kind": "http",
        "status": 200,
        "method": "GET",
        "url": "http://example.test/path"
    });
    assert!(session_matches_filter(&session, "example"));
    assert!(session_matches_filter(&session, "GET"));
    assert!(session_matches_filter(&session, "7"));
    assert!(!session_matches_filter(&session, "missing"));
}

#[test]
fn detail_tab_parse_and_cycle() {
    assert_eq!(DetailTab::parse("headers").unwrap(), DetailTab::Headers);
    assert_eq!(DetailTab::Headers.next(), DetailTab::Body);
    assert_eq!(DetailTab::Headers.previous(), DetailTab::Overview);
    assert!(DetailTab::parse("bad").is_err());
}

#[test]
fn truncate_keeps_short_strings_and_marks_long_strings() {
    assert_eq!(truncate("abc", 8), "abc");
    assert_eq!(truncate("abcdef", 4), "a...");
    assert_eq!(truncate("abcdef", 3), "...");
}

#[test]
fn ratatui_frame_renders_status_table_detail_and_footer_without_overlap() {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    let snapshot = TuiSnapshot {
        status: serde_json::json!({
            "status": "running",
            "proxy": "127.0.0.1:8899",
            "storage": "/tmp/rsproxy",
            "trace": {
                "sessions": 1,
                "spilled": 0,
                "dropped": 0,
                "spill_compression": "none",
                "spill_errors": 0
            }
        }),
        sessions: vec![serde_json::json!({
            "id": 1,
            "kind": "http",
            "status": 200,
            "duration_ms": 4,
            "response_bytes": 12,
            "method": "GET",
            "url": "http://example.test/path"
        })],
        selected_detail: Some(serde_json::json!({
            "id": 1,
            "req_headers": [["Host", "example.test"]],
            "req_trailers": [],
            "res_headers": [["Content-Type", "text/plain"]],
            "res_trailers": []
        })),
        error: None,
    };
    let app = TuiApp {
        api: "127.0.0.1:8900".to_string(),
        limit: 20,
        selected: 0,
        filter: "example".to_string(),
        editing_filter: true,
        detail_tab: DetailTab::Headers,
        replay_status: Some("replayed id=1 status=200".to_string()),
        snapshot,
        last_refresh: std::time::Instant::now(),
    };
    let backend = TestBackend::new(120, 30);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|frame| render_frame(frame, &app)).unwrap();

    let buffer = terminal.backend().buffer();
    let rendered = buffer
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    for expected in [
        "Status",
        "Recent Sessions",
        "Detail: headers",
        "example.test",
        "replayed id=1 status=200",
    ] {
        assert!(
            rendered.contains(expected),
            "missing {expected}: {rendered}"
        );
    }
}

#[test]
fn app_refresh_and_replay_use_live_control_api_and_preserve_selection() {
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let api = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        for _ in 0..4 {
            let (stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            reader.read_line(&mut first).unwrap();
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            let path = first.split_whitespace().nth(1).unwrap();
            let body = if path == "/api/status" {
                r#"{"status":"running","trace":{"sessions":2}}"#
            } else if path.starts_with("/api/sessions?limit=") {
                r#"[{"id":7,"kind":"http","status":200,"duration_ms":1,"response_bytes":4,"method":"GET","url":"http://example.test/"},{"id":8,"kind":"http","status":404,"duration_ms":2,"response_bytes":0,"method":"GET","url":"http://other.test/"}]"#
            } else if path == "/api/sessions/7" {
                r#"{"id":7,"upstream":"example.test:80","flags":[],"error":null}"#
            } else if path == "/api/replay/7" {
                r#"{"id":7,"status":201}"#
            } else {
                panic!("unexpected TUI API path: {path}");
            };
            let mut writer = stream;
            write!(
                writer,
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
    });

    let mut app = TuiApp {
        api,
        limit: 1,
        selected: 4,
        filter: "example".to_string(),
        editing_filter: false,
        detail_tab: DetailTab::Overview,
        replay_status: None,
        snapshot: TuiSnapshot {
            status: serde_json::Value::Null,
            sessions: Vec::new(),
            selected_detail: None,
            error: None,
        },
        last_refresh: std::time::Instant::now(),
    };
    app.refresh();
    assert_eq!(app.selected, 0);
    assert_eq!(app.snapshot.sessions.len(), 1);
    assert_eq!(app.snapshot.sessions[0]["id"], 7);
    assert_eq!(app.snapshot.selected_detail.as_ref().unwrap()["id"], 7);
    app.replay_selected();
    assert_eq!(
        app.replay_status.as_deref(),
        Some("replayed id=7 status=201")
    );
    server.join().unwrap();

    let mut empty = TuiApp {
        snapshot: TuiSnapshot {
            status: serde_json::Value::Null,
            sessions: Vec::new(),
            selected_detail: None,
            error: None,
        },
        ..app
    };
    empty.replay_selected();
    assert_eq!(empty.replay_status.as_deref(), Some("no session selected"));

    let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
    let unavailable_api = unavailable.local_addr().unwrap().to_string();
    drop(unavailable);
    empty.api = unavailable_api;
    empty.refresh();
    assert!(empty.snapshot.error.as_deref().unwrap().contains("connect"));
    assert!(empty.snapshot.sessions.is_empty());
}
