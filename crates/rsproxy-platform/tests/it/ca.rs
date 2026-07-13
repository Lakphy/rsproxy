use rsproxy_platform::PlatformError;
use rsproxy_platform::ca::*;
use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_DIRECTORY: AtomicU64 = AtomicU64::new(1);

fn temporary_ca_directory(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "rsproxy-platform-{name}-{}-{}",
        std::process::id(),
        NEXT_DIRECTORY.fetch_add(1, Ordering::Relaxed)
    ))
}

#[test]
fn generated_root_ca_is_parseable_pem() {
    let root = generate_root_ca("rsproxy-test-root").unwrap();

    assert!(root.certificate_pem.contains("BEGIN CERTIFICATE"));
    assert!(root.private_key_pem.contains("BEGIN PRIVATE KEY"));
    assert!(certificate_fingerprint_sha256(&root.certificate_pem).is_some());
}

#[test]
fn leaf_cache_name_is_path_safe() {
    assert_eq!(leaf_cache_name("API.Example.TEST"), "api.example.test");
    assert_eq!(leaf_cache_name("127.0.0.1:443"), "127.0.0.1_443");
    assert_eq!(leaf_cache_name("*.example.test"), "_.example.test");
}

#[test]
fn root_and_leaf_storage_report_typed_state() {
    let ca_directory = temporary_ca_directory("storage");
    let initialization =
        initialize_root_ca(&ca_directory, "rsproxy platform storage root", false).unwrap();
    assert!(matches!(initialization, CaInitialization::Created { .. }));
    assert!(matches!(
        initialize_root_ca(&ca_directory, "ignored", false).unwrap(),
        CaInitialization::AlreadyInitialized { .. }
    ));

    let status = root_ca_status(&ca_directory).unwrap();
    assert!(status.initialized);
    assert_eq!(status.leaf_cached, 0);
    let root = read_root_ca(&ca_directory).unwrap();
    let stored = store_leaf_certificate(
        &ca_directory,
        "API.Example.TEST",
        &root.certificate_pem,
        &root.private_key_pem,
        &root.certificate_pem,
    )
    .unwrap();
    assert_eq!(stored.paths, leaf_paths(&ca_directory, "api.example.test"));
    assert_eq!(
        cached_leaf_certificate(&ca_directory, "API.Example.TEST")
            .unwrap()
            .unwrap(),
        stored
    );
    assert_eq!(root_ca_status(&ca_directory).unwrap().leaf_cached, 1);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&stored.paths.private_key)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let _ = fs::remove_dir_all(ca_directory);
}

#[test]
fn missing_root_certificate_retains_io_source_and_path_context() {
    let ca_directory = temporary_ca_directory("missing-root");
    let error = read_root_certificate(&ca_directory).unwrap_err();

    match error {
        PlatformError::Io { context, source } => {
            assert!(context.contains("rsproxy-root-ca.pem"));
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected typed I/O failure, got {other:?}"),
    }
}

#[cfg(target_os = "macos")]
#[test]
fn trust_operations_validate_explicit_keychain_before_security_calls() {
    let ca_directory = temporary_ca_directory("trust");
    initialize_root_ca(&ca_directory, "rsproxy platform trust root", false).unwrap();
    let missing = ca_directory.join("missing.keychain-db");
    let options = TrustOptions {
        keychain: Some(missing.clone()),
        dry_run: false,
    };

    assert!(
        install_root_ca(&ca_directory, &options)
            .unwrap_err()
            .to_string()
            .contains("keychain not found")
    );
    assert!(
        uninstall_root_ca(&ca_directory, &options)
            .unwrap_err()
            .to_string()
            .contains("keychain not found")
    );
    assert!(
        keychain_contains_fingerprint(&missing, "00:11")
            .unwrap_err()
            .to_string()
            .contains("keychain not found")
    );
    let _ = fs::remove_dir_all(ca_directory);
}
