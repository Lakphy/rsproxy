use super::support::*;
use serde_json::Value;
use std::fs;

#[test]
fn trace_export_clear_replay_and_tui_once_use_the_control_api() {
    let storage = unique_temp_dir("product-online");
    fs::create_dir_all(&storage).unwrap();

    let clear = run_online(
        &storage,
        &["trace", "clear", "--json"],
        vec![ExpectedResponse {
            method: "POST",
            path: "/api/trace/clear",
            body: r#"{"cleared":2}"#,
        }],
    );
    assert!(assert_success("trace clear", &clear).contains("\"cleared\":2"));

    let export = run_online(
        &storage,
        &["trace", "export", "--json"],
        vec![ExpectedResponse {
            method: "GET",
            path: "/api/sessions/export.json",
            body: "[]",
        }],
    );
    assert_eq!(assert_success("trace export JSON", &export).trim(), "[]");

    let har_path = storage.join("sessions.har");
    let har = run_online(
        &storage,
        &[
            "trace",
            "export",
            "--har",
            "--output",
            har_path.to_str().unwrap(),
        ],
        vec![ExpectedResponse {
            method: "GET",
            path: "/api/sessions/export.har",
            body: r#"{"log":{"version":"1.2","entries":[]}}"#,
        }],
    );
    assert_success("trace export HAR", &har);
    assert!(
        fs::read_to_string(&har_path)
            .unwrap()
            .contains("\"version\":\"1.2\"")
    );

    let replay = run_online(
        &storage,
        &["replay", "7", "--json"],
        vec![ExpectedResponse {
            method: "POST",
            path: "/api/replay/7",
            body: r#"{"replayed":7,"status":204}"#,
        }],
    );
    assert!(assert_success("replay", &replay).contains("\"replayed\":7"));

    let tui = run_online(
        &storage,
        &["tui", "--once", "--limit", "5"],
        vec![
            ExpectedResponse {
                method: "GET",
                path: "/api/status",
                body: r#"{"status":"running","proxy":"127.0.0.1:8899","api":"test","storage":"test","trace":{"sessions":0,"spilled":0,"dropped":0,"spill_compression":"none","spill_errors":0}}"#,
            },
            ExpectedResponse {
                method: "GET",
                path: "/api/sessions?limit=25",
                body: "[]",
            },
        ],
    );
    let snapshot = assert_success("tui once", &tui);
    assert!(snapshot.contains("RSPROXY TUI SNAPSHOT"));
    assert!(snapshot.contains("status=running"));

    let tui_json = run_online(
        &storage,
        &["tui", "--once", "--json"],
        vec![
            ExpectedResponse {
                method: "GET",
                path: "/api/status",
                body: r#"{"status":"running","trace":{"sessions":0}}"#,
            },
            ExpectedResponse {
                method: "GET",
                path: "/api/sessions?limit=100",
                body: "[]",
            },
        ],
    );
    let tui_json = assert_success("tui once JSON", &tui_json);
    let tui_json: Value = serde_json::from_str(&tui_json).unwrap();
    assert_eq!(tui_json["status"]["status"], "running");
    assert!(tui_json["sessions"].as_array().unwrap().is_empty());

    let _ = fs::remove_dir_all(storage);
}
