use super::{mutation, replay, rule_mutation, status, trace_detail, trace_stats};

#[test]
fn human_status_highlights_operational_fields_and_json_stays_exact() {
    let body = r#"{"status":"running","version":"1.2.3","uptime_ms":1250,"proxy":"127.0.0.1:8899","api":"unix:/tmp/ctl.sock","api_auth":{"mode":"peer"},"storage":"/tmp/rsproxy","config":null,"rules":2,"rule_groups":[{},{}],"rule_watch":{"enabled":true,"reloads":3,"failures":1},"mitm":{"mode":"auto","failure_cache_entries":2},"dns":{"mode":"system","lookups":4,"failures":1},"trace":{"sessions":5,"pending_sessions":1,"dropped":2,"queue_dropped":3,"total_memory_bytes":2048,"spilled":4,"spill_bytes":1024}}"#;
    let human = status(body, false).unwrap();
    assert!(human.contains("status=running version=1.2.3 uptime=1.2s"));
    assert!(human.contains("rules=2 groups=2 watch=on reloads=3 failures=1"));
    assert!(human.contains("trace sessions=5 pending=1 dropped=5 memory=2.0KiB"));
    assert_eq!(status(body, true).unwrap(), body);
}

#[test]
fn human_trace_is_sectioned_and_bounds_body_output() {
    let long_body = "x".repeat(2_100);
    let body = serde_json::json!({
        "id": 7, "kind": "http", "status": 201, "method": "POST",
        "url": "http://example.test/", "client": "local", "upstream": "example.test:80",
        "duration_ms": 5, "request_bytes": 3, "response_bytes": 2100,
        "rules": [{"group":"default", "line":1, "raw":"example.test status(201)"}],
        "req_headers": [["Host", "example.test"]], "res_headers": [],
        "res_body_head": long_body
    })
    .to_string();
    let human = trace_detail(&body, false).unwrap();
    assert!(human.contains("id=7 kind=http status=201 method=POST"));
    assert!(human.contains("Matched rules\n  default:1 example.test status(201)"));
    assert!(human.contains("Request headers\n  Host: example.test"));
    assert!(human.contains("truncated: showing 2048 of 2100 characters"));
    assert_eq!(trace_detail(&body, true).unwrap(), body);
}

#[test]
fn rule_mutations_are_human_readable() {
    assert_eq!(
        rule_mutation(r#"{"ok":true,"groups":1,"rules":3}"#, "updated", "default").unwrap(),
        "updated rule group default: 3 rule(s) active"
    );
}

#[test]
fn operational_results_have_human_and_json_forms() {
    assert_eq!(
        mutation(r#"{"ok":true}"#, false, "saved value token").unwrap(),
        "saved value token"
    );
    assert_eq!(
        mutation(r#"{"cleared":2}"#, false, "cleared captured sessions").unwrap(),
        "cleared captured sessions: 2 removed"
    );
    let stats = trace_stats(
        r#"{"sessions":1,"total_memory_bytes":1024,"memory_budget_bytes":2048,"spill_compression":"zstd:3"}"#,
        false,
    )
    .unwrap();
    assert!(stats.contains("sessions=1 pending=0 incomplete=0"));
    assert!(stats.contains("memory=1.0KiB / 2.0KiB"));
    let replayed = replay(
        r#"{"id":7,"url":"http://example.test/","status":204,"response_bytes":0,"headers":[],"body_head":""}"#,
        false,
    )
    .unwrap();
    assert!(replayed.contains("replayed id=7 status=204 bytes=0"));
}
