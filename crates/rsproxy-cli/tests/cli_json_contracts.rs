use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn offline_query_commands_emit_stable_json_shapes() {
    let storage = unique_temp_dir("json-offline");
    fs::create_dir_all(storage.join("rules")).unwrap();
    fs::create_dir_all(storage.join("values")).unwrap();
    fs::write(
        storage.join("rules/default.rules"),
        "contract.test status(204)\n",
    )
    .unwrap();
    fs::write(storage.join("values/alpha"), "one").unwrap();

    let check = run_json(
        &storage,
        &["rules", "check", "--json"],
        Some("contract.test status(204)\n"),
    );
    assert_shape(&check, &json!({"ok": true, "rules": 1}));

    let groups = run_json(&storage, &["rules", "ls", "--json"], None);
    assert_shape(
        &groups,
        &json!([{"enabled": true, "name": "default", "order": 0, "rules": 1}]),
    );

    let group = run_json(&storage, &["rules", "cat", "default", "--json"], None);
    assert_shape(
        &group,
        &json!({"name": "default", "text": "contract.test status(204)\n"}),
    );

    let stats = run_json(&storage, &["rules", "stats", "--json"], None);
    assert_exact_keys(
        &stats,
        &[
            "disabled",
            "domain_exact_entries",
            "domain_suffix_entries",
            "global_rules",
            "indexed_rules",
            "prefilter_literals",
            "prefilter_rules",
            "rules",
        ],
    );
    assert!(stats.as_object().unwrap().values().all(Value::is_number));

    let explain = run_json(
        &storage,
        &["rules", "test", "http://contract.test/", "--json"],
        None,
    );
    assert_exact_keys(&explain, &["explain", "phase", "url"]);
    assert_eq!(explain["phase"], "request");
    assert!(explain["explain"].as_str().unwrap().contains("status(204)"));

    let bench = run_json(
        &storage,
        &[
            "rules",
            "bench",
            "--url",
            "http://contract.test/",
            "--iterations",
            "2",
            "--warmup",
            "0",
            "--json",
        ],
        None,
    );
    assert_exact_keys(
        &bench,
        &[
            "global_rules",
            "indexed_rules",
            "iterations",
            "matched_actions",
            "max_ns",
            "p50_ns",
            "p99_ns",
            "prefilter_literals",
            "prefilter_rules",
            "rules",
            "warmup",
        ],
    );
    assert_eq!(bench["iterations"], 2);

    let values = run_json(&storage, &["values", "ls", "--json"], None);
    assert_eq!(values, json!(["alpha"]));
    let value = run_json(&storage, &["values", "cat", "alpha", "--json"], None);
    assert_eq!(value, json!({"key": "alpha", "value": "one"}));

    let ca = run_json(&storage, &["ca", "status", "--json"], None);
    assert_exact_keys(
        &ca,
        &[
            "ca_dir",
            "cert",
            "cert_path",
            "fingerprint_sha256",
            "initialized",
            "installed",
            "key",
            "key_path",
            "keychain",
            "leaf_cached",
        ],
    );
    assert_eq!(ca["initialized"], false);

    let init = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
        .args(["ca", "init", "--storage"])
        .arg(&storage)
        .env("RSPROXY_HOME", &storage)
        .output()
        .unwrap();
    assert_success("ca init", &init);
    let trust_plan = run_json(
        &storage,
        &[
            "ca",
            "install",
            "--keychain",
            "/tmp/rsproxy-contract.keychain",
            "--dry-run",
            "--json",
        ],
        None,
    );
    assert_exact_keys(&trust_plan, &["args", "dry_run", "platform", "program"]);
    assert_eq!(trust_plan["dry_run"], true);

    let _ = fs::remove_dir_all(storage);
}

#[test]
fn online_query_commands_preserve_json_documents() {
    for (args, path, body, expected) in [
        (
            vec!["status", "--json"],
            "/api/status",
            r#"{"status":"running","proxy":"127.0.0.1:8899"}"#,
            json!({"proxy": "127.0.0.1:8899", "status": "running"}),
        ),
        (
            vec!["trace", "ls", "--json"],
            "/api/sessions?limit=20",
            r#"[{"id":7,"kind":"http"}]"#,
            json!([{"id": 7, "kind": "http"}]),
        ),
        (
            vec!["trace", "get", "7", "--json"],
            "/api/sessions/7",
            r#"{"id":7,"kind":"http","status":204}"#,
            json!({"id": 7, "kind": "http", "status": 204}),
        ),
        (
            vec!["trace", "stats", "--json"],
            "/api/trace/stats",
            r#"{"sessions":1,"dropped":0}"#,
            json!({"dropped": 0, "sessions": 1}),
        ),
    ] {
        let (api, server) = serve_once(path, body);
        let output = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .args(&args)
            .args(["--api", &api])
            .env("RSPROXY_HOME", unique_temp_dir("json-online-home"))
            .output()
            .unwrap();
        server.join().unwrap();
        assert_success(&args.join(" "), &output);
        let actual: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn json_errors_are_single_stable_documents() {
    let storage = unique_temp_dir("json-errors");
    fs::create_dir_all(&storage).unwrap();
    fs::write(storage.join("broken.toml"), "port = 'not-a-number'\n").unwrap();
    let broken_config = storage.join("broken.toml").display().to_string();
    let cases = [
        vec!["unknown", "--json"],
        vec!["rules", "test", "--json"],
        vec![
            "status",
            "--api",
            "127.0.0.1:1",
            "--storage",
            storage.to_str().unwrap(),
            "--json",
        ],
        vec!["status", "--config", &broken_config, "--json"],
    ];

    for args in cases {
        let output = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .args(&args)
            .env("RSPROXY_HOME", &storage)
            .output()
            .unwrap();
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr)
            .unwrap_or_else(|parse| panic!("{args:?}: {parse}: {}", stderr(&output)));
        assert_eq!(error["schema"], "rsproxy.cli.error/v1");
        assert_eq!(error["ok"], false);
        assert_eq!(error["error"]["code"], "command_failed");
        assert!(error["error"]["message"].is_string());
        assert_exact_keys(&error, &["error", "ok", "schema"]);
        assert_exact_keys(&error["error"], &["code", "message"]);
    }
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn system_proxy_dry_run_status_is_structured_on_every_platform() {
    for (platform, extra) in [
        ("macos", Some(("--service", "Contract Service"))),
        ("windows", None),
        ("linux", None),
    ] {
        let mut args = vec![
            "proxy",
            "status",
            "--platform",
            platform,
            "--dry-run",
            "--json",
        ];
        if let Some((name, value)) = extra {
            args.extend([name, value]);
        }
        let output = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .args(&args)
            .env("RSPROXY_HOME", unique_temp_dir("proxy-json-home"))
            .output()
            .unwrap();
        assert_success(platform, &output);
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_exact_keys(&value, &["commands", "dry_run", "platform"]);
        assert_eq!(value["platform"], platform);
        assert_eq!(value["dry_run"], true);
        assert!(!value["commands"].as_array().unwrap().is_empty());
    }
}

fn run_json(storage: &Path, args: &[&str], stdin: Option<&str>) -> Value {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rsproxy"));
    command
        .args(args)
        .args([
            "--storage",
            storage.to_str().unwrap(),
            "--api",
            "127.0.0.1:1",
        ])
        .env("RSPROXY_HOME", storage)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if stdin.is_some() {
        command.stdin(Stdio::piped());
    }
    let mut child = command.spawn().unwrap();
    if let Some(stdin) = stdin {
        child
            .stdin
            .take()
            .unwrap()
            .write_all(stdin.as_bytes())
            .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert_success(&args.join(" "), &output);
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|error| panic!("{args:?}: {error}: {}", stdout(&output)))
}

fn serve_once(
    expected_path: &'static str,
    body: &'static str,
) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let api = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        assert!(
            request_line.starts_with(&format!("GET {expected_path} HTTP/1.1")),
            "unexpected request: {request_line}"
        );
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        stream
            .write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .as_bytes(),
            )
            .unwrap();
    });
    (api, server)
}

fn assert_shape(actual: &Value, expected: &Value) {
    assert_eq!(json_shape(actual), json_shape(expected));
}

fn json_shape(value: &Value) -> Value {
    match value {
        Value::Null => json!("null"),
        Value::Bool(_) => json!("boolean"),
        Value::Number(_) => json!("number"),
        Value::String(_) => json!("string"),
        Value::Array(values) => json!({
            "type": "array",
            "items": values.first().map(json_shape).unwrap_or_else(|| json!("unknown")),
        }),
        Value::Object(values) => Value::Object(
            values
                .iter()
                .map(|(key, value)| (key.clone(), json_shape(value)))
                .collect(),
        ),
    }
}

fn assert_exact_keys(value: &Value, expected: &[&str]) {
    let mut actual = value
        .as_object()
        .expect("JSON value should be an object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    actual.sort_unstable();
    assert_eq!(actual, expected);
}

fn assert_success(label: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{label}: stdout={} stderr={}",
        stdout(output),
        stderr(output)
    );
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rsproxy-{label}-{}-{nonce}", std::process::id()))
}
