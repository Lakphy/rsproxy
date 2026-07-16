use super::*;
use std::fs;

#[test]
fn manifest_round_trips_without_inline_credentials() {
    let manifest = StartupManifest {
        version: STARTUP_MANIFEST_VERSION,
        storage: PathBuf::from("/tmp/rsproxy"),
        config: Some(PathBuf::from("/tmp/rsproxy/config.toml")),
        system_proxy: true,
        service: Some("Wi-Fi".to_string()),
        bypass: Some(vec!["localhost".to_string()]),
        proxy_host: "127.0.0.1".to_string(),
        proxy_port: 8899,
    };
    let encoded = serde_json::to_vec(&manifest).unwrap();
    let decoded = manifest::parse_manifest(&encoded).unwrap();
    assert_eq!(decoded.storage, manifest.storage);
    assert_eq!(decoded.service, manifest.service);
    assert!(!String::from_utf8(encoded).unwrap().contains("api_token"));
}

#[test]
fn bypass_parser_trims_and_ignores_empty_entries() {
    assert_eq!(
        system_proxy::parse_bypass_list(Some(" localhost, ,*.example.test ")),
        Some(vec!["localhost".to_string(), "*.example.test".to_string()])
    );
}

#[test]
fn lenient_reads_degrade_corrupt_manifests_to_a_warning() {
    let path = env::temp_dir().join(format!(
        "rsproxy-manifest-corrupt-{}.json",
        std::process::id()
    ));
    fs::write(&path, b"{not json").unwrap();
    let (manifest, warning) = read_manifest_lenient(&path);
    assert!(manifest.is_none());
    assert!(warning.unwrap().contains("unreadable"));
    let _ = fs::remove_file(&path);
}

#[test]
fn lenient_reads_treat_missing_manifests_as_unconfigured() {
    let path = env::temp_dir().join(format!(
        "rsproxy-manifest-missing-{}.json",
        std::process::id()
    ));
    let (manifest, warning) = read_manifest_lenient(&path);
    assert!(manifest.is_none());
    assert!(warning.is_none());
}

#[test]
fn unknown_manifest_versions_are_rejected() {
    let error = manifest::parse_manifest(
        br#"{"version":2,"storage":"/tmp","config":null,"system_proxy":false,"service":null,"bypass":null,"proxy_host":"127.0.0.1","proxy_port":8899}"#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("version 2 is unsupported"));
}

#[test]
fn current_platform_name_is_stable() {
    assert!(!platform_name(rsproxy_platform::startup::current_startup_platform()).is_empty());
}
