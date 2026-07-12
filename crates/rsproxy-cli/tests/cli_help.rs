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
    assert!(stdout.contains("rsproxy run|start|stop|restart"));
    assert!(stdout.contains("--watch"));
    assert!(stdout.contains("--watch-debounce-ms MS"));
    assert!(stdout.contains("rsproxy rules"));
    assert!(stdout.contains("rsproxy rules enable|disable <group>"));
    assert!(stdout.contains("--response-status CODE"));
    assert!(stdout.contains("--response-header 'Name: value'"));
    assert!(stdout.contains("rsproxy trace"));
    assert!(stdout.contains("rsproxy completions"));
    assert!(stdout.contains("rsproxy --version"));
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
        (&["rules", "test", "--help"], "--response-status CODE"),
        (&["values", "--help"], "rsproxy values"),
        (&["values", "ls", "--help"], "values ls"),
        (&["values", "cat", "--help"], "values cat"),
        (&["values", "set", "--help"], "values set"),
        (&["values", "rm", "--help"], "values rm"),
        (&["trace", "--help"], "rsproxy trace"),
        (&["trace", "ls", "--help"], "trace ls"),
        (&["trace", "get", "--help"], "trace get"),
        (&["trace", "follow", "--help"], "--count N"),
        (&["trace", "stats", "--help"], "trace stats"),
        (&["trace", "clear", "--help"], "trace clear"),
        (&["trace", "export", "--help"], "trace export"),
        (&["tui", "--help"], "rsproxy tui"),
        (&["replay", "--help"], "rsproxy replay"),
        (&["ca", "--help"], "rsproxy ca"),
        (&["ca", "init", "--help"], "ca init"),
        (&["ca", "status", "--help"], "ca status"),
        (&["ca", "export", "--help"], "ca export"),
        (&["ca", "issue", "--help"], "rsproxy ca issue <HOST>"),
        (&["ca", "install", "--help"], "ca install"),
        (&["ca", "uninstall", "--help"], "ca uninstall"),
        (&["proxy", "--help"], "rsproxy proxy"),
        (&["proxy", "status", "--help"], "proxy status"),
        (&["proxy", "on", "--help"], "rsproxy proxy on|off"),
        (&["proxy", "off", "--help"], "rsproxy proxy on|off"),
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
