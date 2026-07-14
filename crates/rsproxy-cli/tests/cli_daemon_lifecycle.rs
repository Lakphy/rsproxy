//! Process-level daemon lifecycle integration tests.
#![allow(clippy::unwrap_used)]

#[path = "cli_daemon_lifecycle/support.rs"]
mod support;

use serde_json::Value;
use std::fs;
use std::net::TcpListener;
use std::process::Command;
#[cfg(windows)]
use std::time::{SystemTime, UNIX_EPOCH};
use support::*;

#[test]
fn daemon_lifecycle_recovers_stale_state_and_preserves_rules() {
    let mut harness = DaemonHarness::new();
    let rules_dir = harness.storage.join("rules");
    fs::create_dir_all(&rules_dir).unwrap();
    fs::write(
        rules_dir.join("default.rules"),
        "lifecycle.test status(204)\n",
    )
    .unwrap();

    let start = harness.start();
    assert_success("start", &start);
    assert!(stdout(&start).contains("started pid="));
    let first_pid = harness.pid();

    let duplicate = harness.run("start");
    assert!(!duplicate.status.success());
    assert_eq!(duplicate.status.code(), Some(3));
    assert!(stderr(&duplicate).contains("already running"));

    let status = status_json(&harness);
    assert_eq!(status["status"], "running");
    assert_eq!(status["rules"], 1);
    assert_eq!(status["rule_groups"][0]["name"], "default");
    assert_eq!(status["api_auth"]["mode"], "token");

    let restart = harness.run("restart");
    assert_success("restart", &restart);
    let restarted_pid = harness.pid();
    assert_ne!(restarted_pid, first_pid);
    assert_eq!(status_json(&harness)["rules"], 1);

    force_terminate(restarted_pid);
    wait_for_exit(restarted_pid);
    assert!(harness.pid_path().is_file(), "abnormal exit keeps pidfile");

    let recovered = harness.start();
    assert_success("start after abnormal exit", &recovered);
    assert_ne!(harness.pid(), restarted_pid);
    assert_eq!(status_json(&harness)["rules"], 1);

    let stop = harness.run("stop");
    assert_success("stop", &stop);
    assert!(!harness.pid_path().exists());

    fs::create_dir_all(harness.pid_path().parent().unwrap()).unwrap();
    fs::write(harness.pid_path(), "not-a-pid\n").unwrap();
    let recovered = harness.start();
    assert_success("start after malformed pidfile", &recovered);
    assert!(status_json(&harness)["uptime_ms"].is_number());

    let stop = harness.run("stop");
    assert_success("final stop", &stop);
    let stopped_status = harness.run("status");
    assert!(!stopped_status.status.success());
    assert!(stderr(&stopped_status).contains("connect"));
}

#[test]
fn daemon_mode_rejects_ephemeral_ports_without_leaving_state() {
    let storage = unique_temp_dir("daemon-ephemeral");
    for args in [
        vec!["--port", "0", "--api", "127.0.0.1:19191"],
        vec!["--port", "19190", "--api", "127.0.0.1:0"],
    ] {
        let output = command_output(
            Command::new(env!("CARGO_BIN_EXE_rsproxy"))
                .arg("start")
                .args(args)
                .arg("--storage")
                .arg(&storage),
        );
        assert!(!output.status.success());
        assert_eq!(output.status.code(), Some(2));
        assert!(stderr(&output).contains("non-zero"));
        assert!(!storage.join("run/rsproxy.pid").exists());
    }
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn daemon_start_fails_cleanly_when_the_proxy_listener_is_occupied() {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_port = occupied.local_addr().unwrap().port().to_string();
    let api = format!("127.0.0.1:{}", unused_port());
    let storage = unique_temp_dir("daemon-bind-failure");
    let output = command_output(
        Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .arg("start")
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &proxy_port,
                "--api",
                &api,
                "--storage",
            ])
            .arg(&storage),
    );

    assert!(!output.status.success());
    // The bind failure is surfaced directly in the CLI error instead of only in the daemon log.
    assert!(stderr(&output).contains("bind proxy listener"));
    assert!(!storage.join("run/rsproxy.pid").exists());
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn stop_reclaims_an_orphaned_run_process_holding_the_proxy_port() {
    let storage = unique_temp_dir("daemon-orphan-reclaim");
    let proxy_port = unused_port();
    let mut api_port = unused_port();
    while api_port == proxy_port {
        api_port = unused_port();
    }
    let api = format!("127.0.0.1:{api_port}");
    let proxy = proxy_port.to_string();

    // A foreground `run` writes no pidfile, so a leaked instance is invisible to a pidfile-based
    // stop; the port-owner recovery path must find and reclaim it.
    let orphan = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
        .arg("run")
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &proxy,
            "--api",
            &api,
            "--storage",
        ])
        .arg(&storage)
        .args(["--no-mitm", "--trace-disk-budget", "0"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wait_until_listening(proxy_port);
    assert!(
        !storage.join("run/rsproxy.pid").exists(),
        "run writes no pidfile"
    );

    // In production the orphan reparents to init, which reaps it once stop signals it. Here the
    // orphan is our child, so reap it in the background — an unreaped zombie still answers
    // `kill -0`, which would make stop believe it never exited.
    let reaper = std::thread::spawn(move || {
        let mut orphan = orphan;
        let _ = orphan.wait();
    });

    let stop = command_output(
        Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .arg("stop")
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &proxy,
                "--api",
                &api,
                "--storage",
            ])
            .arg(&storage),
    );
    assert_success("stop reclaim", &stop);
    assert!(stdout(&stop).contains("reclaimed"));
    assert!(
        TcpListener::bind(("127.0.0.1", proxy_port)).is_ok(),
        "port freed"
    );

    reaper.join().unwrap();
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn stop_refuses_to_reclaim_a_foreign_process_holding_the_proxy_port() {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let proxy_port = occupied.local_addr().unwrap().port().to_string();
    let api = format!("127.0.0.1:{}", unused_port());
    let storage = unique_temp_dir("daemon-foreign-port");

    // The test harness itself holds the port; stop must not signal an unrelated process.
    let output = command_output(
        Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .arg("stop")
            .args([
                "--host",
                "127.0.0.1",
                "--port",
                &proxy_port,
                "--api",
                &api,
                "--storage",
            ])
            .arg(&storage),
    );

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert!(stderr(&output).contains("not rsproxy"));
    assert!(occupied.local_addr().is_ok(), "listener retained");
    let _ = fs::remove_dir_all(storage);
}

#[cfg(unix)]
#[test]
fn run_shuts_down_when_its_supervisor_dies() {
    let storage = unique_temp_dir("daemon-watchdog");
    let proxy_port = unused_port();
    let mut api_port = unused_port();
    while api_port == proxy_port {
        api_port = unused_port();
    }
    let api = format!("127.0.0.1:{api_port}");
    let proxy = proxy_port.to_string();

    // A stand-in for the npm shim: run watches this pid and must exit when it dies, covering the
    // uncatchable-SIGKILL case that signal forwarding alone cannot.
    let mut supervisor = Command::new("sleep").arg("60").spawn().unwrap();

    let run = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
        .arg("run")
        .args([
            "--host",
            "127.0.0.1",
            "--port",
            &proxy,
            "--api",
            &api,
            "--storage",
        ])
        .arg(&storage)
        .args(["--no-mitm", "--trace-disk-budget", "0"])
        .env("RSPROXY_SUPERVISOR_PID", supervisor.id().to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let run_pid = run.id();
    let reaper = std::thread::spawn(move || {
        let mut run = run;
        let _ = run.wait();
    });

    wait_until_listening(proxy_port);
    let _ = supervisor.kill();
    let _ = supervisor.wait();

    wait_for_exit(run_pid);
    assert!(
        TcpListener::bind(("127.0.0.1", proxy_port)).is_ok(),
        "port freed after watchdog shutdown"
    );
    reaper.join().unwrap();
    let _ = fs::remove_dir_all(storage);
}

#[cfg(unix)]
#[test]
fn stop_refuses_to_terminate_an_unverified_pidfile_process() {
    let storage = unique_temp_dir("daemon-unverified-pid");
    fs::create_dir_all(storage.join("run")).unwrap();
    let mut unrelated = Command::new("sleep").arg("30").spawn().unwrap();
    fs::write(storage.join("run/rsproxy.pid"), unrelated.id().to_string()).unwrap();
    let api = format!("127.0.0.1:{}", unused_port());
    let output = command_output(
        Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .arg("stop")
            .args(["--api", &api, "--storage"])
            .arg(&storage),
    );

    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(3));
    assert!(stderr(&output).contains("refusing to terminate"));
    assert!(unrelated.try_wait().unwrap().is_none());
    let _ = unrelated.kill();
    let _ = unrelated.wait();
    let _ = fs::remove_dir_all(storage);
}

#[cfg(unix)]
#[test]
fn unix_daemon_defaults_to_a_private_storage_local_control_socket() {
    use std::os::unix::fs::PermissionsExt;

    let storage = unique_temp_dir("unix-default-control");
    let run = |command: &str, proxy_port: u16| {
        let port = proxy_port.to_string();
        let mut rsproxy = Command::new(env!("CARGO_BIN_EXE_rsproxy"));
        rsproxy.arg(command);
        if command == "status" {
            rsproxy.arg("--json");
        }
        command_output(
            rsproxy
                .args(["--host", "127.0.0.1", "--port", &port, "--storage"])
                .arg(&storage)
                .args(["--no-mitm", "--trace-disk-budget", "0"]),
        )
    };

    let (proxy_port, start) = start_daemon_on_probed_port(&storage, |port| run("start", port));
    assert_success("Unix default control start", &start);
    let socket = expected_default_unix_socket(&storage);
    assert!(socket.exists());
    assert_eq!(
        fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let status = run("status", proxy_port);
    assert_success("Unix default control status", &status);
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["api_auth"]["mode"], "peer");
    assert!(status["api"].as_str().unwrap().starts_with("unix:"));
    let stop = command_output(
        Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .arg("stop")
            .arg("--storage")
            .arg(&storage),
    );
    assert_success("Unix default control stop", &stop);
    assert!(!socket.exists());
    let _ = fs::remove_dir_all(storage);
}

#[cfg(windows)]
#[test]
fn windows_daemon_uses_the_authenticated_named_pipe_control_plane() {
    let storage = unique_temp_dir("windows-pipe-daemon");
    let pipe = format!(
        "pipe:rsproxy-test-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let run = |command: &str, proxy_port: u16| {
        let port = proxy_port.to_string();
        command_output(
            Command::new(env!("CARGO_BIN_EXE_rsproxy"))
                .arg(command)
                .args([
                    "--host",
                    "127.0.0.1",
                    "--port",
                    &port,
                    "--api",
                    &pipe,
                    "--storage",
                ])
                .arg(&storage)
                .args(["--no-mitm", "--trace-disk-budget", "0"]),
        )
    };

    let (proxy_port, start) = start_daemon_on_probed_port(&storage, |port| run("start", port));
    assert_success("Windows named pipe start", &start);
    let status = run("status", proxy_port);
    assert_success("Windows named pipe status", &status);
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["status"], "running");
    assert_eq!(status["api_auth"]["mode"], "token");
    assert!(status["api"].as_str().unwrap().starts_with("pipe:"));
    let stop = run("stop", proxy_port);
    assert_success("Windows named pipe stop", &stop);
    let _ = fs::remove_dir_all(storage);
}
