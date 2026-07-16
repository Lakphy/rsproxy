#![cfg(unix)]

use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_HOME: AtomicU64 = AtomicU64::new(1);

fn run(home: &std::path::Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_rsproxy"))
        .args(args)
        .env("HOME", home)
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env_remove("RSPROXY_HOME")
        .output()
        .expect("startup command should execute")
}

#[test]
fn startup_registration_round_trips_in_an_isolated_home() {
    let home = std::env::temp_dir().join(format!(
        "rsproxy-startup-it-{}-{}",
        std::process::id(),
        NEXT_HOME.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&home).unwrap();

    let install = run(
        &home,
        &["startup", "install", "--no-system-proxy", "--json"],
    );
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let installed: serde_json::Value = serde_json::from_slice(&install.stdout).unwrap();
    assert_eq!(installed["action"], "install");
    assert_eq!(installed["system_proxy"], false);

    let status = run(&home, &["startup", "status", "--json"]);
    assert!(status.status.success());
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["installed"], true);
    assert_eq!(status["configured"], true);
    assert_eq!(status["system_proxy"], false);

    let uninstall = run(&home, &["startup", "uninstall", "--keep-running", "--json"]);
    assert!(
        uninstall.status.success(),
        "{}",
        String::from_utf8_lossy(&uninstall.stderr)
    );

    let status = run(&home, &["startup", "status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["installed"], false);
    assert_eq!(status["configured"], false);

    let _ = std::fs::remove_dir_all(home);
}

#[test]
fn startup_start_now_and_cleanup_keep_json_as_single_documents() {
    let home = std::env::temp_dir().join(format!(
        "rsproxy-startup-now-it-{}-{}",
        std::process::id(),
        NEXT_HOME.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&home).unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let config = home.join("config.toml");
    std::fs::write(
        &config,
        format!("port = {port}\nstorage = {:?}\n", home.join("state")),
    )
    .unwrap();

    let install = run(
        &home,
        &[
            "startup",
            "install",
            "--config",
            config.to_str().unwrap(),
            "--no-system-proxy",
            "--start-now",
            "--json",
        ],
    );
    assert!(
        install.status.success(),
        "{}",
        String::from_utf8_lossy(&install.stderr)
    );
    let installed: serde_json::Value = serde_json::from_slice(&install.stdout).unwrap();
    assert_eq!(installed["action"], "install");
    assert_eq!(installed["start_now"], true);

    let launch = run(&home, &["startup", "launch", "--json"]);
    assert!(
        launch.status.success(),
        "{}",
        String::from_utf8_lossy(&launch.stderr)
    );
    let launched: serde_json::Value = serde_json::from_slice(&launch.stdout).unwrap();
    assert_eq!(launched["action"], "launch");
    assert_eq!(launched["system_proxy"], false);

    let uninstall = run(&home, &["startup", "uninstall", "--json"]);
    assert!(
        uninstall.status.success(),
        "{}",
        String::from_utf8_lossy(&uninstall.stderr)
    );
    let uninstalled: serde_json::Value = serde_json::from_slice(&uninstall.stdout).unwrap();
    assert_eq!(uninstalled["action"], "uninstall");
    assert_eq!(uninstalled["stop_runtime"], true);

    let _ = std::fs::remove_dir_all(home);
}
