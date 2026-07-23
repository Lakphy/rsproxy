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

fn success(output: Output) -> String {
    assert!(
        output.status.success(),
        "command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
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
fn offline_cli_manages_ordered_rule_groups_end_to_end() {
    let storage = temp_storage("rule-groups");

    success(run(
        &storage,
        &["rules", "set", "default"],
        Some("@language 3\nexample.test status(201)"),
    ));
    success(run(
        &storage,
        &["rules", "set", "override"],
        Some("@language 3\nexample.test status(202) @important"),
    ));

    let list = success(run(&storage, &["rules", "ls", "--json"], None));
    let groups: serde_json::Value = serde_json::from_str(list.trim()).unwrap();
    assert_eq!(groups[0]["name"], "default");
    assert_eq!(groups[1]["name"], "override");
    assert_eq!(groups[1]["enabled"], true);

    success(run(&storage, &["rules", "disable", "override"], None));
    let fallback = success(run(
        &storage,
        &["rules", "test", "http://example.test/"],
        None,
    ));
    assert!(fallback.contains("default:2 status(201)"));
    assert!(!fallback.contains("override:2 status(202)"));

    success(run(&storage, &["rules", "enable", "override"], None));
    let overridden = success(run(
        &storage,
        &["rules", "test", "http://example.test/"],
        None,
    ));
    assert!(overridden.contains("override:2 status(202)"));

    success(run(&storage, &["rules", "rm", "override"], None));
    let list = success(run(&storage, &["rules", "ls", "--json"], None));
    let groups: serde_json::Value = serde_json::from_str(list.trim()).unwrap();
    assert_eq!(groups.as_array().unwrap().len(), 1);
    assert_eq!(groups[0]["name"], "default");

    success(run(
        &storage,
        &["rules", "set", "default"],
        Some("@language 3\nexample.test res.header(x-template: ${statusCode}|${resH.x-origin})"),
    ));
    let response_explain = success(run(
        &storage,
        &[
            "rules",
            "test",
            "http://example.test/",
            "--response-status",
            "202",
            "--response-header",
            "X-Origin: upstream",
        ],
        None,
    ));
    assert!(response_explain.contains("res.header(x-template: 202|upstream)"));

    let invalid = run(&storage, &["rules", "edit", "../escape"], None);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("invalid rule group name"));

    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn rules_test_keeps_sibling_regexes_when_root_path_is_optional() {
    let storage = temp_storage("overlapping-regex");
    success(run(
        &storage,
        &["rules", "set", "root"],
        Some("@language 3\n/^https?:\\/\\/h\\.example\\.com\\/?$/ direct"),
    ));
    success(run(
        &storage,
        &["rules", "set", "asset"],
        Some("@language 3\n/^https?:\\/\\/h\\.example\\.com\\/(.+)\\.js$/ direct"),
    ));

    let asset = success(run(
        &storage,
        &["rules", "test", "https://h.example.com/a.js"],
        None,
    ));
    assert!(asset.contains("asset:2 direct"), "stdout: {asset}");

    let root = success(run(
        &storage,
        &["rules", "test", "https://h.example.com/"],
        None,
    ));
    assert!(root.contains("root:2 direct"), "stdout: {root}");

    let _ = std::fs::remove_dir_all(storage);
}
