use super::support::{request, response_body, test_state};
use crate::control::routes::dispatch;
use rsproxy_rules::{Action, RequestMeta};
use std::fs;
use std::io::{BufRead, BufReader};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn rule_request() -> RequestMeta {
    RequestMeta {
        method: "GET".to_string(),
        url: "http://example.test/".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    }
}

#[test]
fn dispatches_status_and_rules_without_changing_route_contracts() {
    let mut state = test_state();
    state.config.config_path = Some(state.config.storage.join("config.toml"));
    state.config.rules_watch = true;
    state.config.rules_watch_debounce = std::time::Duration::from_millis(75);

    let mut status = Vec::new();
    dispatch(&mut status, &request("GET", "/api/status", &[]), &state).unwrap();
    let status_text = std::str::from_utf8(&status).unwrap();
    assert!(status_text.starts_with("HTTP/1.1 200 OK\r\n"));
    let status_json: serde_json::Value = serde_json::from_str(response_body(&status)).unwrap();
    assert_eq!(status_json["status"], "running");
    assert_eq!(status_json["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status_json["api_auth"]["mode"], "peer");
    assert_eq!(
        status_json["trace"]["queue_capacity"],
        rsproxy_trace::DEFAULT_TRACE_QUEUE_CAPACITY
    );
    assert_eq!(status_json["trace"]["queue_dropped"], 0);
    assert_eq!(
        status_json["trace"]["memory_budget_bytes"],
        rsproxy_trace::DEFAULT_TRACE_MEMORY_BUDGET
    );
    assert_eq!(status_json["trace"]["evicted_sessions"], 0);
    assert_eq!(
        status_json["body_buffer_limit"],
        state.config.body_buffer_limit
    );
    assert_eq!(
        status_json["config"],
        state
            .config
            .storage
            .join("config.toml")
            .display()
            .to_string()
    );
    assert!(status_json.get("api_token").is_none());
    assert_eq!(status_json["rule_groups"][0]["name"], "default");
    assert_eq!(status_json["rule_groups"][0]["enabled"], true);
    assert_eq!(status_json["rule_watch"]["enabled"], true);
    assert_eq!(status_json["rule_watch"]["debounce_ms"], 75);
    assert_eq!(status_json["rule_watch"]["events"], 0);
    assert_eq!(status_json["rule_watch"]["dropped_events"], 0);
    assert_eq!(status_json["mitm"]["mode"], "auto");
    assert_eq!(status_json["mitm"]["failure_cache_entries"], 0);
    assert_eq!(status_json["mitm"]["failure_ttl_ms"], 300_000);
    assert_eq!(status_json["mitm"]["connect_probe_timeout_ms"], 250);
    assert_eq!(
        status_json["rule_watch"]["last_error"],
        serde_json::Value::Null
    );

    let rules = b"example.test status(209)";
    let mut set = Vec::new();
    dispatch(
        &mut set,
        &request("POST", "/api/rules/default", rules),
        &state,
    )
    .unwrap();
    assert!(
        std::str::from_utf8(&set)
            .unwrap()
            .starts_with("HTTP/1.1 200 OK\r\n")
    );

    let mut get = Vec::new();
    dispatch(&mut get, &request("GET", "/api/rules/default", &[]), &state).unwrap();
    assert_eq!(response_body(&get), "example.test status(209)");

    let mut invalid = Vec::new();
    dispatch(
        &mut invalid,
        &request("POST", "/api/rules/default", b"example.test unknown()"),
        &state,
    )
    .unwrap();
    assert!(
        std::str::from_utf8(&invalid)
            .unwrap()
            .starts_with("HTTP/1.1 400 Bad Request\r\n")
    );

    let mut invalid_header = Vec::new();
    dispatch(
        &mut invalid_header,
        &request(
            "GET",
            "/api/rules/test?url=http%3A%2F%2Fexample.test%2F&responseHeader=missing-colon",
            &[],
        ),
        &state,
    )
    .unwrap();
    assert!(
        std::str::from_utf8(&invalid_header)
            .unwrap()
            .starts_with("HTTP/1.1 400 Bad Request\r\n")
    );
    let invalid_json: serde_json::Value = serde_json::from_str(response_body(&invalid)).unwrap();
    assert_eq!(invalid_json["errors"][0]["code"], "action");
    assert_eq!(invalid_json["errors"][0]["group"], "default");
    assert_eq!(invalid_json["errors"][0]["line"], 1);
    assert_eq!(
        state.rules.snapshot().group("default").unwrap().text,
        "example.test status(209)"
    );

    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn live_session_route_streams_collector_broadcasts_and_observes_disconnects() {
    let state = test_state();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(2)))
        .unwrap();
    let (mut server, _) = listener.accept().unwrap();
    let server_state = state.clone();
    let worker = std::thread::spawn(move || {
        let _ = dispatch(
            &mut server,
            &request(
                "GET",
                "/api/sessions/follow?after=0&limit=8&heartbeat_ms=100",
                &[],
            ),
            &server_state,
        );
    });

    let mut reader = BufReader::new(client.try_clone().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if line == "\r\n" {
            break;
        }
    }

    state.trace.record(rsproxy_trace::Session::new(
        rsproxy_trace::SessionKind::Http,
        "GET".to_string(),
        "http://example.test/live".to_string(),
        "127.0.0.1:12345".to_string(),
    ));
    loop {
        line.clear();
        reader.read_line(&mut line).unwrap();
        if !line.trim().is_empty() {
            break;
        }
    }
    let session: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(session["url"], "http://example.test/live");

    drop(reader);
    client.shutdown(std::net::Shutdown::Both).unwrap();
    worker.join().unwrap();
    assert_eq!(state.trace.stats().follow_subscribers, 0);
}

#[test]
fn rules_test_route_accepts_and_validates_response_context() {
    let state = test_state();
    let rule = b"example.test res.header(x-template: ${statusCode}|${resH.x-origin})";
    let mut set = Vec::new();
    dispatch(
        &mut set,
        &request("POST", "/api/rules/default", rule),
        &state,
    )
    .unwrap();

    let mut explained = Vec::new();
    dispatch(
        &mut explained,
        &request(
            "GET",
            "/api/rules/test?url=http%3A%2F%2Fexample.test%2F&responseStatus=202&responseHeader=X-Origin%3A%20upstream",
            &[],
        ),
        &state,
    )
    .unwrap();
    assert_eq!(
        response_body(&explained),
        "default:1 res.header(x-template: 202|upstream)\n"
    );

    let mut invalid = Vec::new();
    dispatch(
        &mut invalid,
        &request(
            "GET",
            "/api/rules/test?url=http%3A%2F%2Fexample.test%2F&responseStatus=700",
            &[],
        ),
        &state,
    )
    .unwrap();
    assert!(
        std::str::from_utf8(&invalid)
            .unwrap()
            .starts_with("HTTP/1.1 400 Bad Request\r\n")
    );

    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn named_rule_group_routes_publish_one_ordered_snapshot() {
    let state = test_state();
    for (path, text) in [
        ("/api/rules/default", "example.test status(201)"),
        ("/api/rules/override", "example.test status(202) @important"),
    ] {
        let mut response = Vec::new();
        dispatch(
            &mut response,
            &request("POST", path, text.as_bytes()),
            &state,
        )
        .unwrap();
        assert!(
            std::str::from_utf8(&response)
                .unwrap()
                .starts_with("HTTP/1.1 200 OK\r\n")
        );
    }

    let snapshot = state.rules.snapshot();
    assert_eq!(snapshot.groups.len(), 2);
    assert!(matches!(
        snapshot.compiled.resolve(&rule_request()).actions[0].action,
        Action::Status(202)
    ));
    drop(snapshot);

    let mut list = Vec::new();
    dispatch(&mut list, &request("GET", "/api/rules", &[]), &state).unwrap();
    let groups: serde_json::Value = serde_json::from_str(response_body(&list)).unwrap();
    assert_eq!(groups[0]["name"], "default");
    assert_eq!(groups[1]["name"], "override");
    assert_eq!(groups[1]["enabled"], true);

    let mut disable = Vec::new();
    dispatch(
        &mut disable,
        &request("POST", "/api/rules/override/disable", &[]),
        &state,
    )
    .unwrap();
    assert!(matches!(
        state
            .rules
            .snapshot()
            .compiled
            .resolve(&rule_request())
            .actions[0]
            .action,
        Action::Status(201)
    ));

    let mut export = Vec::new();
    dispatch(
        &mut export,
        &request("GET", "/api/rules/export", &[]),
        &state,
    )
    .unwrap();
    let exported: serde_json::Value = serde_json::from_str(response_body(&export)).unwrap();
    assert_eq!(exported[1]["enabled"], false);
    assert_eq!(exported[1]["text"], "example.test status(202) @important");

    let mut remove = Vec::new();
    dispatch(
        &mut remove,
        &request("DELETE", "/api/rules/override", &[]),
        &state,
    )
    .unwrap();
    assert!(state.rules.snapshot().group("override").is_none());
    assert!(!state.config.storage.join("rules/override.rules").exists());

    let _ = fs::remove_dir_all(&state.config.storage);
}
