use serde_json::Value;
use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

struct ChildGuard(Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn run_emits_machine_readable_listener_and_startup_events() {
    let storage = unique_temp_dir("logging");
    let mut child = ChildGuard(
        Command::new(env!("CARGO_BIN_EXE_rsproxy"))
            .args([
                "run",
                "--host",
                "127.0.0.1",
                "--port",
                "0",
                "--api",
                "127.0.0.1:0",
                "--storage",
                storage.to_str().unwrap(),
                "--no-mitm",
                "--trace-disk-budget",
                "0",
            ])
            .env("RSPROXY_LOG", "rsproxy_cli=info")
            .env("RUST_LOG", "off")
            .env("RSPROXY_LOG_FORMAT", "json")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("rsproxy should start"),
    );
    let stderr = child.0.stderr.take().expect("stderr should be piped");
    let (sender, receiver) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines() {
            let _ = sender.send(line);
        }
    });

    let wanted = [
        "daemon_started",
        "proxy_listener_bound",
        "control_listener_bound",
        "upstream_trust_roots_loaded",
    ];
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut events = BTreeMap::new();
    let mut parse_errors = Vec::new();
    let mut raw_lines = Vec::new();
    while Instant::now() < deadline && events.len() < wanted.len() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let Ok(line) = receiver.recv_timeout(remaining.min(Duration::from_millis(250))) else {
            continue;
        };
        match line {
            Ok(line) => match serde_json::from_str::<Value>(&line) {
                Ok(value) => {
                    raw_lines.push(line);
                    let event = value
                        .pointer("/fields/event")
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    if wanted.contains(&event) {
                        events.insert(event.to_string(), value);
                    }
                }
                Err(error) => parse_errors.push(format!("{error}: {line}")),
            },
            Err(error) => parse_errors.push(error.to_string()),
        }
    }

    drop(child);
    reader.join().unwrap();
    let _ = std::fs::remove_dir_all(&storage);

    assert!(
        parse_errors.is_empty(),
        "invalid JSON logs: {parse_errors:?}"
    );
    for event in wanted {
        assert!(
            events.contains_key(event),
            "missing log event {event}; received keys={:?}; lines={raw_lines:?}",
            events.keys().collect::<Vec<_>>()
        );
    }
    for event in ["proxy_listener_bound", "control_listener_bound"] {
        let address = events[event]
            .pointer("/fields/address")
            .and_then(Value::as_str)
            .expect("listener event should expose address")
            .parse::<SocketAddr>()
            .expect("listener address should be a socket address");
        assert_ne!(address.port(), 0);
    }
    assert_eq!(
        events["control_listener_bound"]
            .pointer("/fields/transport")
            .and_then(Value::as_str),
        Some("tcp")
    );
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("rsproxy-{label}-{}-{nonce}", std::process::id()))
}
