use super::support::*;
use serde_json::Value;
use std::fs;

#[test]
fn values_crud_and_ca_lifecycle_work_through_the_real_binary() {
    let storage = unique_temp_dir("product-offline");

    assert_success(
        "values set",
        &run_offline(&storage, &["values", "set", "alpha"], Some("one")),
    );
    let values = assert_success(
        "values list",
        &run_offline(&storage, &["values", "ls", "--json"], None),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&values).unwrap(),
        serde_json::json!(["alpha"])
    );
    assert_eq!(
        assert_success(
            "values cat",
            &run_offline(&storage, &["values", "cat", "alpha"], None)
        ),
        "one"
    );
    assert_success(
        "values remove",
        &run_offline(&storage, &["values", "rm", "alpha"], None),
    );
    assert!(
        !run_offline(&storage, &["values", "cat", "alpha"], None)
            .status
            .success()
    );

    assert_success("ca init", &run_offline(&storage, &["ca", "init"], None));
    let status = assert_success(
        "ca status",
        &run_offline(&storage, &["ca", "status", "--json"], None),
    );
    assert_eq!(
        serde_json::from_str::<Value>(&status).unwrap()["initialized"],
        true
    );

    let exported = storage.join("exported-ca.pem");
    assert_success(
        "ca export",
        &run_offline(
            &storage,
            &["ca", "export", "--out", exported.to_str().unwrap()],
            None,
        ),
    );
    assert!(
        fs::read_to_string(&exported)
            .unwrap()
            .contains("BEGIN CERTIFICATE")
    );
    assert_success(
        "ca issue",
        &run_offline(&storage, &["ca", "issue", "matrix.test"], None),
    );
    assert!(storage.join("ca/leaf/matrix.test.pem").is_file());

    let keychain = storage.join("contract.keychain");
    assert_success(
        "ca install dry-run",
        &run_offline(
            &storage,
            &[
                "ca",
                "install",
                "--dry-run",
                "--keychain",
                keychain.to_str().unwrap(),
            ],
            None,
        ),
    );
    assert_success(
        "ca uninstall dry-run",
        &run_offline(
            &storage,
            &[
                "ca",
                "uninstall",
                "--dry-run",
                "--keychain",
                keychain.to_str().unwrap(),
            ],
            None,
        ),
    );

    let _ = fs::remove_dir_all(storage);
}

#[test]
fn every_system_proxy_platform_has_structured_mutation_plans() {
    let storage = unique_temp_dir("proxy-product-matrix");
    for platform in ["macos", "windows", "linux"] {
        for action in ["on", "off"] {
            let mut args = vec![
                "proxy",
                action,
                "--platform",
                platform,
                "--host",
                "127.0.0.1",
                "--port",
                "18888",
                "--dry-run",
                "--json",
            ];
            if platform == "macos" {
                args.extend(["--service", "Contract Service"]);
            }
            let body = assert_success(
                &format!("proxy {platform} {action}"),
                &run_offline(&storage, &args, None),
            );
            let value: Value = serde_json::from_str(&body).unwrap();
            assert_eq!(value["platform"], platform);
            assert_eq!(value["dry_run"], true);
            assert!(!value["commands"].as_array().unwrap().is_empty());
        }
    }
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn command_family_errors_use_the_json_error_contract() {
    let storage = unique_temp_dir("product-errors");
    for args in [
        vec!["rules", "unknown", "--json"],
        vec!["values", "cat", "--json"],
        vec!["trace", "get", "--json"],
        vec!["ca", "unknown", "--json"],
        vec!["proxy", "unknown", "--json"],
        vec!["completions", "unknown", "--json"],
    ] {
        let output = run_offline(&storage, &args, None);
        assert!(!output.status.success(), "{args:?} unexpectedly succeeded");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["schema"], "rsproxy.cli.error/v1");
        assert!(error["error"]["message"].is_string());
    }
    let _ = fs::remove_dir_all(storage);
}
