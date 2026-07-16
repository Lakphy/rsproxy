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
        "*.foo.test upstream(socks5://127.0.0.1:1111)\napi.foo.test upstream(socks5://127.0.0.1:2222)\n",
    )
    .unwrap();

    let output = run(
        &storage,
        &["rules", "lint", "--file", rules_path.to_str().unwrap()],
        None,
    );
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("never wins upstream"), "stdout: {stdout}");
    assert!(stdout.contains("default:1"), "stdout: {stdout}");

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
    let stdout = String::from_utf8(output.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap()).unwrap();
    assert_eq!(value["ok"], serde_json::json!(false));
    assert_eq!(value["findings"][0]["line"], serde_json::json!(2));
    assert_eq!(
        value["findings"][0]["families"],
        serde_json::json!(["upstream"])
    );

    std::fs::remove_dir_all(&storage).ok();
}

#[test]
fn rules_lint_passes_when_specific_rules_come_first() {
    let storage = temp_storage("rules-lint-ok");
    let output = run(
        &storage,
        &["rules", "set", "default"],
        Some(
            "api.foo.test upstream(socks5://127.0.0.1:2222)\n*.foo.test upstream(socks5://127.0.0.1:1111)\n",
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
    assert!(stdout.contains("no shadowed rules"), "stdout: {stdout}");
    std::fs::remove_dir_all(&storage).ok();
}
