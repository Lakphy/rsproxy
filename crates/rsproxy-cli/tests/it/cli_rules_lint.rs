use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

fn run(storage: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_rsproxy"));
    command
        .args(args)
        .arg("--storage")
        .arg(storage)
        .arg("--api")
        .arg("127.0.0.1:1")
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
    child.wait_with_output().unwrap()
}

fn temp_storage(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "rsproxy-cli-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

#[test]
fn rules_lint_reports_shadowed_rules_and_exits_nonzero() {
    let storage = temp_storage("rules-lint");
    let rules_path = storage.join("shadow.rules");
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::write(
        &rules_path,
        "@language 3\n*.foo.test upstream(socks5://127.0.0.1:1111)\napi.foo.test upstream(socks5://127.0.0.1:2222)\n",
    )
    .unwrap();

    let output = run(
        &storage,
        &["rules", "lint", "--file", rules_path.to_str().unwrap()],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("never wins upstream"), "stdout: {stdout}");
    assert!(stdout.contains("default:3"), "stdout: {stdout}");

    let output = run(
        &storage,
        &[
            "rules",
            "lint",
            "--file",
            rules_path.to_str().unwrap(),
            "--json",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert_eq!(value["schema"], "rsproxy.rules.lint/v1");
    assert_eq!(value["ok"], serde_json::json!(false));
    assert_eq!(value["complete"], serde_json::json!(true));
    assert!(value["shadow_comparisons"].as_u64().unwrap() > 0);
    assert!(value["shadow_comparison_bytes"].as_u64().unwrap() > 0);
    assert_eq!(value["limits"]["comparisons"], 1_000_000);
    assert_eq!(value["limits"]["comparison_bytes"], 268_435_456);
    assert_eq!(value["limits"]["findings_per_report"], 10_000);
    assert_eq!(value["limits"]["report_bytes"], 4_194_304);
    assert_eq!(value["findings"][0]["kind"], "shadowed-rule");
    assert_eq!(value["findings"][0]["line"], serde_json::json!(3));
    assert_eq!(
        value["findings"][0]["families"],
        serde_json::json!(["upstream"])
    );

    std::fs::remove_dir_all(&storage).ok();
}

#[test]
fn rules_lint_reports_incomplete_without_fabricating_a_finding() {
    let storage = temp_storage("rules-lint-incomplete");
    let rules_path = storage.join("incomplete.rules");
    std::fs::create_dir_all(&storage).unwrap();
    let source = std::iter::once("@language 3".to_string())
        .chain((0..1415).map(|index| format!("=http://host-{index}.test/ status(200)")))
        .collect::<Vec<_>>()
        .join("\n");
    std::fs::write(&rules_path, source).unwrap();

    let output = run(
        &storage,
        &[
            "rules",
            "lint",
            "--file",
            rules_path.to_str().unwrap(),
            "--json",
        ],
        None,
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], serde_json::json!(false));
    assert_eq!(value["complete"], serde_json::json!(false));
    assert_eq!(value["findings"], serde_json::json!([]));
    std::fs::remove_dir_all(&storage).ok();
}

#[test]
fn rules_lint_passes_when_specific_rules_come_first() {
    let storage = temp_storage("rules-lint-ok");
    let output = run(
        &storage,
        &["rules", "set", "default"],
        Some(
            "@language 3\napi.foo.test upstream(socks5://127.0.0.1:2222)\n*.foo.test upstream(socks5://127.0.0.1:1111)\n",
        ),
    );
    assert!(output.status.success());

    let output = run(&storage, &["rules", "lint"], None);
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("no rule lint findings"), "stdout: {stdout}");
    std::fs::remove_dir_all(&storage).ok();
}

#[test]
fn rules_lint_reports_same_rule_semantic_findings() {
    let storage = temp_storage("rules-lint-semantic");
    let rules_path = storage.join("semantic.rules");
    std::fs::create_dir_all(&storage).unwrap();
    std::fs::write(
        &rules_path,
        concat!(
            "@language 3\n",
            "example.test status(201) status(202)\n",
            "example.test direct when method(GET) when all(method(POST), header(x))\n",
            "example.test req.header(x: y) when status(404)\n"
        ),
    )
    .unwrap();

    let output = run(
        &storage,
        &[
            "rules",
            "lint",
            "--file",
            rules_path.to_str().unwrap(),
            "--json",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "rsproxy.rules.lint/v1");
    assert_eq!(value["complete"], serde_json::json!(true));
    assert_eq!(value["findings"][0]["kind"], "duplicate-single-family");
    assert_eq!(
        value["findings"][0]["families"],
        serde_json::json!(["status"])
    );
    assert_eq!(value["findings"][1]["kind"], "unsatisfiable-conditions");
    assert!(
        value["findings"][1]["families"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        value["findings"][2]["kind"],
        "request-action-requires-response"
    );
    assert_eq!(
        value["findings"][2]["families"],
        serde_json::json!(["req.header"])
    );

    let output = run(
        &storage,
        &["rules", "lint", "--file", rules_path.to_str().unwrap()],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(
        stdout.contains("duplicate-single-family"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("unsatisfiable-conditions"),
        "stdout: {stdout}"
    );
    assert!(
        stdout.contains("request-action-requires-response"),
        "stdout: {stdout}"
    );

    std::fs::remove_dir_all(&storage).ok();
}

#[test]
fn map_remote_check_and_mitm_advisories_cover_websocket_migrations() {
    let storage = temp_storage("map-remote-websocket");
    std::fs::create_dir_all(&storage).unwrap();
    let websocket_rules = concat!(
        "@language 3\n",
        "/^wss?:\\/\\/socket\\.test\\/(.*)$/ ",
        "map.remote(wss://127.0.0.1:3000/$1)\n"
    );
    let rules_path = storage.join("websocket.rules");
    std::fs::write(&rules_path, websocket_rules).unwrap();

    let output = run(
        &storage,
        &["rules", "check", rules_path.to_str().unwrap()],
        None,
    );
    assert!(output.status.success());

    let invalid_path = storage.join("invalid.rules");
    std::fs::write(
        &invalid_path,
        "@language 3\nsocket.test map.remote(socks5://127.0.0.1:1080/$1)\n",
    )
    .unwrap();
    let output = run(
        &storage,
        &["rules", "check", invalid_path.to_str().unwrap()],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("must use http, https, ws, or wss"));

    let output = run(
        &storage,
        &[
            "rules",
            "set",
            "default",
            "--file",
            rules_path.to_str().unwrap(),
        ],
        None,
    );
    assert!(output.status.success());

    let output = run(&storage, &["rules", "test", "wss://socket.test/live"], None);
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("map.remote(wss://127.0.0.1:3000/live)"));
    assert!(stdout.contains("warning[https-mitm-unavailable]"));
    assert!(stdout.contains("rsproxy ca init && rsproxy ca install"));

    let output = run(&storage, &["rules", "lint", "--json"], None);
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], serde_json::json!(true));
    assert_eq!(
        value["warnings"][0]["kind"],
        serde_json::json!("https-mitm-unavailable")
    );

    let output = run(&storage, &["ca", "init"], None);
    assert!(output.status.success());
    let output = run(&storage, &["rules", "test", "wss://socket.test/live"], None);
    assert!(output.status.success());
    assert!(
        !String::from_utf8(output.stdout)
            .unwrap()
            .contains("https-mitm-unavailable")
    );

    std::fs::remove_dir_all(&storage).ok();
}
