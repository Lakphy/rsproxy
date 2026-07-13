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
        "completions",
    ] {
        assert!(stdout.contains(command), "root help omitted {command}");
    }
    assert!(stdout.contains("Usage: rsproxy"));
    assert!(stdout.contains("--version"));

    let runtime = command_output(&["run", "--help"]);
    assert!(runtime.status.success());
    let runtime = String::from_utf8(runtime.stdout).unwrap();
    assert!(runtime.contains("--watch"));
    assert!(runtime.contains("--watch-debounce-ms"));
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
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(expected),
            "{args:?} did not contain {expected:?}"
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
