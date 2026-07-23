use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

fn command_output(args: &[&str]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("rsproxy binary should start");
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child
            .try_wait()
            .expect("rsproxy status should be readable")
            .is_some()
        {
            return child
                .wait_with_output()
                .expect("rsproxy output should be readable");
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("rsproxy {args:?} did not exit within ten seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn help_command_exposes_the_supported_entry_points() {
    let output = command_output(&["help"]);

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("help should be UTF-8");
    for command in [
        "run",
        "start",
        "stop",
        "restart",
        "status",
        "rules",
        "values",
        "trace",
        "tui",
        "replay",
        "ca",
        "proxy",
        "startup",
        "config",
        "completions",
    ] {
        assert!(stdout.contains(command), "root help omitted {command}");
    }
    assert!(stdout.contains("Usage: rsproxy"));
    assert!(stdout.contains("--version"));
    for guidance in [
        "QUICK START:",
        "rsproxy ca init",
        "Preview CA trust changes:",
        "rsproxy proxy on --all --dry-run",
        "rsproxy proxy off --all",
        "CONFIGURATION:",
    ] {
        assert!(stdout.contains(guidance), "root help omitted {guidance}");
    }

    let runtime = command_output(&["run", "--help"]);
    assert!(runtime.status.success());
    let runtime = String::from_utf8(runtime.stdout).unwrap();
    assert!(runtime.contains("--watch"));
    assert!(runtime.contains("--watch-debounce-ms"));
}

#[test]
fn trace_help_marks_json_before_piping_to_jq() {
    for args in [&["trace", "--help"][..], &["trace", "get", "--help"][..]] {
        let output = command_output(args);
        assert!(output.status.success(), "{args:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("trace get 42 --json | jq"),
            "{args:?}: {stdout}"
        );
        assert!(!stdout.contains("trace get 42 | jq"), "{args:?}: {stdout}");
    }
}

#[test]
fn rules_language_help_is_indexed_searchable_machine_readable_and_runtime_free() {
    let topic = command_output(&["rules", "help", "req.header"]);
    assert!(topic.status.success());
    let topic = String::from_utf8(topic.stdout).unwrap();
    for expected in [
        "action.req.header",
        "req.header(NAME: VALUE)",
        "This family stacks",
        "action.res.header",
    ] {
        assert!(topic.contains(expected), "topic omitted {expected:?}");
    }

    let search = command_output(&["rules", "help", "--search", "response header"]);
    assert!(search.status.success());
    let search = String::from_utf8(search.stdout).unwrap();
    assert!(search.contains("condition.res.header"));
    assert!(search.contains("action.res.header"));

    let json = command_output(&["rules", "help", "action.tls", "--json"]);
    assert!(json.status.success());
    let json: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(json["schema"], "rsproxy.rules.help/v1");
    assert_eq!(json["language_version"], 3);
    assert_eq!(json["limits"]["source_line_bytes"], 65_536);
    assert_eq!(json["limits"]["snapshot_source_bytes"], 16_777_216);
    assert_eq!(json["limits"]["groups_per_snapshot"], 1_024);
    assert_eq!(json["limits"]["rules_per_snapshot"], 10_000);
    assert_eq!(json["limits"]["diagnostics"], 256);
    assert_eq!(json["limits"]["actions_per_rule"], 256);
    assert_eq!(json["limits"]["actions_per_snapshot"], 100_000);
    assert_eq!(json["limits"]["condition_nodes_per_rule"], 256);
    assert_eq!(json["limits"]["condition_nodes_per_snapshot"], 100_000);
    assert_eq!(json["limits"]["body_conditions_per_snapshot"], 256);
    assert_eq!(json["limits"]["properties_per_rule"], 64);
    assert_eq!(json["limits"]["call_arguments"], 256);
    assert_eq!(json["limits"]["external_value_bytes"], 8_388_608);
    assert_eq!(json["limits"]["rendered_value_bytes"], 8_388_608);
    assert_eq!(json["limits"]["external_path_bytes"], 4_096);
    assert_eq!(json["limits"]["rendered_tag_bytes"], 4_096);
    assert_eq!(json["limits"]["tags_per_request"], 256);
    assert_eq!(json["limits"]["explain_value_bytes"], 4_096);
    assert_eq!(json["limits"]["explain_bytes"], 8_388_608);
    assert_eq!(json["limits"]["upstream_hops"], 32);
    assert_eq!(json["limits"]["mock_file_candidates"], 32);
    assert_eq!(json["limits"]["lint_comparisons"], 1_000_000);
    assert_eq!(json["limits"]["lint_comparison_bytes"], 268_435_456);
    assert_eq!(json["limits"]["lint_findings_per_report"], 10_000);
    assert_eq!(json["limits"]["lint_report_bytes"], 4_194_304);
    assert_eq!(json["limits"]["tls_pem_bytes"], 1_048_576);
    assert_eq!(
        json["limits"]["condition_http_status"],
        serde_json::json!([100, 599])
    );
    assert_eq!(
        json["limits"]["final_http_status"],
        serde_json::json!([200, 599])
    );
    assert_eq!(
        json["limits"]["redirect_status"],
        serde_json::json!([301, 302, 303, 307, 308])
    );
    assert_eq!(json["kind"], "topic");
    assert_eq!(json["topics"][0]["id"], "action.tls");
    assert!(json["topics"][0]["syntax"].as_array().unwrap().len() >= 3);
    assert_eq!(json["topics"][0]["dsl_spellings"][0]["canonical"], "tls");
    assert_eq!(
        json["topics"][0]["resolution"],
        "single: first applicable action wins"
    );

    let ambiguous = command_output(&["rules", "help", "status"]);
    assert_eq!(ambiguous.status.code(), Some(2));
    let error = String::from_utf8(ambiguous.stderr).unwrap();
    assert!(error.contains("action.status"));
    assert!(error.contains("condition.status"));

    let runtime_free = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
        .args([
            "rules",
            "help",
            "concept.rule",
            "--config",
            "/definitely/missing/rsproxy.toml",
        ])
        .env("RSPROXY_LOG_FORMAT", "invalid-for-runtime")
        .output()
        .unwrap();
    assert!(
        runtime_free.status.success(),
        "{}",
        String::from_utf8_lossy(&runtime_free.stderr)
    );
}

#[test]
fn long_help_explains_defaults_inputs_and_safe_workflows() {
    let cases = [
        (
            &["run", "--help"][..],
            &["Proxy listener:", "built-in default: 8mb", "EXAMPLES:"][..],
        ),
        (
            &["rules", "test", "--help"][..],
            &[
                "Absolute request URL",
                "Name: value",
                "Simulate request metadata",
                "network traffic",
            ][..],
        ),
        (
            &["values", "set", "--help"][..],
            &["stdin", "--file", "EXAMPLES:"][..],
        ),
        (
            &["trace", "clear", "--help"][..],
            &["cannot be undone", "does not prompt for confirmation"][..],
        ),
        (
            &["ca", "install", "--help"][..],
            &["elevated privileges", "--dry-run", "Only install a CA"][..],
        ),
        (
            &["proxy", "on", "--help"][..],
            &["--service", "--all", "SAFE WORKFLOW:"][..],
        ),
    ];

    for (args, expected) in cases {
        let output = command_output(args);
        assert!(output.status.success(), "{args:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        for needle in expected {
            assert!(
                stdout.contains(needle),
                "{args:?} omitted {needle:?}; stdout={stdout:?}"
            );
        }
    }
}

#[test]
fn semantic_errors_include_a_recovery_hint_for_humans() {
    let output = command_output(&[
        "rules",
        "test",
        "https://example.test",
        "--response-status",
        "not-a-status",
    ]);

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("error: response status must be numeric"));
    assert!(stderr.contains("hint: run `rsproxy --help`"));
}

#[test]
fn empty_argv_prints_root_help_and_succeeds() {
    let output = command_output(&[]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("Usage: rsproxy"));
    assert!(output.stderr.is_empty());
}

#[test]
fn version_flags_report_the_manifest_version() {
    for flag in ["--version", "-V"] {
        let output = command_output(&[flag]);
        assert!(output.status.success());
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("rsproxy {}\n", env!("CARGO_PKG_VERSION"))
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn every_command_help_exits_without_runtime_side_effects() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-help-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let storage_arg = storage.display().to_string();
    let cases: &[(&[&str], &str)] = &[
        (&["run", "--help", "--storage", &storage_arg], "rsproxy run"),
        (&["start", "-h", "--storage", &storage_arg], "rsproxy start"),
        (
            &["stop", "--help", "--storage", &storage_arg],
            "rsproxy stop",
        ),
        (
            &["restart", "--help", "--storage", &storage_arg],
            "rsproxy restart",
        ),
        (&["status", "--help"], "rsproxy status"),
        (&["rules", "--help"], "rsproxy rules"),
        (&["rules", "help", "--help"], "built-in rule-language index"),
        (&["rules", "check", "--help"], "rules check"),
        (&["rules", "ls", "--help"], "rules ls"),
        (&["rules", "cat", "--help"], "rules cat"),
        (&["rules", "edit", "--help"], "rules edit"),
        (&["rules", "set", "--help"], "rules set"),
        (&["rules", "rm", "--help"], "rules rm"),
        (&["rules", "enable", "--help"], "rules enable"),
        (&["rules", "disable", "--help"], "rules disable"),
        (&["rules", "stats", "--help"], "rules stats"),
        (&["rules", "bench", "--help"], "rules bench"),
        (&["rules", "test", "--help"], "--response-status"),
        (&["values", "--help"], "rsproxy values"),
        (&["values", "ls", "--help"], "values ls"),
        (&["values", "cat", "--help"], "values cat"),
        (&["values", "set", "--help"], "values set"),
        (&["values", "rm", "--help"], "values rm"),
        (&["trace", "--help"], "rsproxy trace"),
        (&["trace", "ls", "--help"], "trace ls"),
        (&["trace", "get", "--help"], "trace get"),
        (&["trace", "follow", "--help"], "--count"),
        (&["trace", "stats", "--help"], "trace stats"),
        (&["trace", "clear", "--help"], "trace clear"),
        (&["trace", "export", "--help"], "trace export"),
        (&["tui", "--help"], "rsproxy tui"),
        (&["replay", "--help"], "rsproxy replay"),
        (&["ca", "--help"], "rsproxy ca"),
        (&["ca", "init", "--help"], "ca init"),
        (&["ca", "status", "--help"], "ca status"),
        (&["ca", "export", "--help"], "ca export"),
        (&["ca", "issue", "--help"], "<HOST>"),
        (&["ca", "install", "--help"], "ca install"),
        (&["ca", "uninstall", "--help"], "ca uninstall"),
        (&["proxy", "--help"], "rsproxy proxy"),
        (&["proxy", "status", "--help"], "proxy status"),
        (&["proxy", "on", "--help"], "proxy on"),
        (&["proxy", "off", "--help"], "proxy off"),
        (&["startup", "--help"], "rsproxy startup"),
        (&["startup", "install", "--help"], "--start-now"),
        (&["startup", "status", "--help"], "startup status"),
        (&["startup", "uninstall", "--help"], "--keep-running"),
        (&["completions", "--help"], "rsproxy completions"),
        (&["help", "rules", "test"], "--response-header"),
    ];

    for (args, expected) in cases {
        let output = command_output(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout).replace("rsproxy.exe", "rsproxy");
        assert!(
            stdout.contains(expected),
            "{args:?} did not contain {expected:?}; stdout={stdout:?}"
        );
    }
    assert!(
        !storage.exists(),
        "help must not load runtime config or create storage"
    );
}

#[test]
fn help_keeps_unknown_command_errors_explicit() {
    for args in [
        &["unknown", "--help"][..],
        &["rules", "unknown", "--help"][..],
    ] {
        let output = command_output(args);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("unknown"));
    }
}

#[test]
fn clap_rejects_unknown_and_unrelated_options_with_usage_exit_code() {
    for args in [
        &["run", "--porrt", "8899"][..],
        &["values", "ls", "--watch"][..],
    ] {
        let output = command_output(args);
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("unexpected argument"));
        assert!(stderr.contains("Usage:"));
    }
}
