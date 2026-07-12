use super::super::*;

#[test]
fn ca_issue_host_skips_options_with_values() {
    let args = vec![
        "issue".to_string(),
        "--config".to_string(),
        "/tmp/rsproxy.toml".to_string(),
        "--storage".to_string(),
        "/tmp/rsproxy".to_string(),
        "--force".to_string(),
        "api.example.test".to_string(),
    ];
    assert_eq!(ca_issue_host(&args).as_deref(), Some("api.example.test"));
}

#[test]
fn generated_root_ca_is_parseable_pem() {
    let (cert, key) = generate_root_ca("rsproxy-test-root").unwrap();

    assert!(cert.contains("BEGIN CERTIFICATE"));
    assert!(key.contains("BEGIN"));
    assert!(cert_fingerprint(&cert).is_some());
}

#[test]
fn leaf_cache_name_is_path_safe() {
    assert_eq!(leaf_cache_name("API.Example.TEST"), "api.example.test");
    assert_eq!(leaf_cache_name("127.0.0.1:443"), "127.0.0.1_443");
    assert_eq!(leaf_cache_name("*.example.test"), "_.example.test");
}

#[test]
fn ca_keychain_argument_rejects_missing_and_option_values() {
    assert_eq!(ca_keychain_arg(&[]).unwrap(), None);
    assert_eq!(
        ca_keychain_arg(&[
            "--keychain".to_string(),
            "/tmp/login.keychain-db".to_string()
        ])
        .unwrap(),
        Some(PathBuf::from("/tmp/login.keychain-db"))
    );
    assert!(ca_keychain_arg(&["--keychain".to_string()]).is_err());
    assert!(ca_keychain_arg(&["--keychain".to_string(), "--json".to_string()]).is_err());
}

#[cfg(target_os = "macos")]
#[test]
fn ca_trust_operations_validate_explicit_keychain_before_security_calls() {
    let ca_dir = std::env::temp_dir().join(format!(
        "rsproxy-ca-trust-test-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    ca_init(&["init".to_string()], &ca_dir).unwrap();
    let missing = ca_dir.join("missing.keychain-db");
    let args = vec![
        "install".to_string(),
        "--keychain".to_string(),
        missing.display().to_string(),
    ];

    assert_eq!(ca_target_keychain(&args).unwrap(), missing);
    assert!(
        ca_install(&args, &ca_dir)
            .unwrap_err()
            .contains("keychain not found")
    );
    assert!(
        ca_uninstall(&args, &ca_dir)
            .unwrap_err()
            .contains("keychain not found")
    );
    assert!(
        ca_keychain_contains_fingerprint(&missing, "00:11")
            .unwrap_err()
            .contains("keychain not found")
    );
    let _ = fs::remove_dir_all(ca_dir);
}

#[cfg(target_os = "macos")]
#[test]
fn security_command_runner_maps_success_failure_and_spawn_errors() {
    let mut success = Command::new("/bin/sh");
    success.args(["-c", "printf 'ok'"]);
    let output = security_output("success", &mut success).unwrap();
    assert_eq!(output.stdout, b"ok");

    let mut failure = Command::new("/bin/sh");
    failure.args(["-c", "printf 'denied' >&2; exit 7"]);
    let error = security_output("failure", &mut failure).unwrap_err();
    assert!(error.contains("failure failed: denied"));

    let mut missing = Command::new("/path/that/does/not/exist/rsproxy-security-test");
    let error = security_raw_output("missing", &mut missing).unwrap_err();
    assert!(error.starts_with("missing:"));
}

#[cfg(target_os = "macos")]
#[test]
fn security_output_helpers_classify_messages() {
    fn output(script: &str) -> std::process::Output {
        Command::new("/bin/sh")
            .args(["-c", script])
            .output()
            .unwrap()
    }

    let both = output("printf 'out'; printf 'err' >&2; exit 1");
    assert_eq!(security_output_message(&both), "err; out");
    assert!(!security_output_is_not_found(&both));

    let stderr = output("printf 'No matching item' >&2; exit 1");
    assert_eq!(security_output_message(&stderr), "No matching item");
    assert!(security_output_is_not_found(&stderr));

    let stdout = output("printf 'certificate could not be found'; exit 1");
    assert_eq!(
        security_output_message(&stdout),
        "certificate could not be found"
    );
    assert!(security_output_is_not_found(&stdout));

    let empty = output("exit 9");
    assert!(security_output_message(&empty).contains("exit status: 9"));
}
