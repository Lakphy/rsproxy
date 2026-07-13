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

fn assert_io_context(error: PlatformError, expected: &str) {
    match error {
        PlatformError::Io { context, .. } => assert!(
            context.contains(expected),
            "expected context containing {expected:?}, got {context:?}"
        ),
        other => panic!("expected typed I/O failure, got {other:?}"),
    }
}

#[test]
fn storage_distinguishes_partial_invalid_and_incomplete_material() {
    assert_eq!(TrustAction::Install.completed_name(), "installed");
    assert_eq!(TrustAction::Uninstall.completed_name(), "uninstalled");

    let ca_directory = temporary_ca_directory("partial-certificate");
    let paths = CaPaths::new(&ca_directory);
    fs::create_dir_all(&ca_directory).unwrap();
    fs::write(&paths.certificate, "not a certificate").unwrap();
    assert!(matches!(
        initialize_root_ca(&ca_directory, "ignored", false),
        Err(PlatformError::InvalidState(message)) if message.contains("partial CA state")
    ));
    let status = root_ca_status(&ca_directory).unwrap();
    assert!(status.certificate_exists);
    assert!(!status.private_key_exists);
    assert!(!status.initialized);
    assert_eq!(status.fingerprint_sha256, "invalid-pem");
    assert_eq!(
        read_root_certificate(&ca_directory).unwrap(),
        "not a certificate"
    );
    assert_io_context(
        read_root_ca(&ca_directory).unwrap_err(),
        "rsproxy-root-ca-key.pem",
    );

    let leaf = leaf_paths(&ca_directory, "Incomplete.Example");
    fs::create_dir_all(leaf.certificate.parent().unwrap()).unwrap();
    fs::write(&leaf.certificate, "not a certificate").unwrap();
    assert_eq!(
        cached_leaf_certificate(&ca_directory, "Incomplete.Example").unwrap(),
        None
    );
    fs::write(&leaf.private_key, "key").unwrap();
    fs::write(&leaf.chain, "chain").unwrap();
    let cached = cached_leaf_certificate(&ca_directory, "Incomplete.Example")
        .unwrap()
        .unwrap();
    assert_eq!(cached.fingerprint_sha256, "unknown");
    assert_eq!(root_ca_status(&ca_directory).unwrap().leaf_cached, 1);
    let _ = fs::remove_dir_all(ca_directory);

    let ca_directory = temporary_ca_directory("partial-key");
    let paths = CaPaths::new(&ca_directory);
    fs::create_dir_all(&ca_directory).unwrap();
    fs::write(&paths.private_key, "key only").unwrap();
    assert!(matches!(
        initialize_root_ca(&ca_directory, "ignored", false),
        Err(PlatformError::InvalidState(_))
    ));
    let forced = initialize_root_ca(&ca_directory, "forced replacement", true).unwrap();
    assert!(matches!(forced, CaInitialization::Created { .. }));
    let _ = fs::remove_dir_all(ca_directory);
}

#[test]
fn storage_write_failures_retain_the_specific_path_context() {
    let ca_directory = temporary_ca_directory("directory-is-file");
    fs::write(&ca_directory, "blocking file").unwrap();
    assert_io_context(
        initialize_root_ca(&ca_directory, "blocked", false).unwrap_err(),
        "create CA directory",
    );
    fs::remove_file(&ca_directory).unwrap();

    let ca_directory = temporary_ca_directory("certificate-is-directory");
    let paths = CaPaths::new(&ca_directory);
    fs::create_dir_all(&paths.certificate).unwrap();
    assert_io_context(
        initialize_root_ca(&ca_directory, "blocked certificate", true).unwrap_err(),
        "rsproxy-root-ca.pem",
    );
    let _ = fs::remove_dir_all(&ca_directory);

    let ca_directory = temporary_ca_directory("private-key-is-directory");
    let paths = CaPaths::new(&ca_directory);
    fs::create_dir_all(&paths.private_key).unwrap();
    assert_io_context(
        initialize_root_ca(&ca_directory, "blocked key", true).unwrap_err(),
        "rsproxy-root-ca-key.pem",
    );
    let _ = fs::remove_dir_all(&ca_directory);

    let ca_directory = temporary_ca_directory("readme-is-directory");
    let paths = CaPaths::new(&ca_directory);
    fs::create_dir_all(&paths.readme).unwrap();
    assert_io_context(
        initialize_root_ca(&ca_directory, "blocked readme", true).unwrap_err(),
        "README.txt",
    );
    let _ = fs::remove_dir_all(&ca_directory);
}

#[test]
fn storage_read_and_leaf_write_failures_are_typed() {
    let ca_directory = temporary_ca_directory("invalid-root-bytes");
    let paths = CaPaths::new(&ca_directory);
    fs::create_dir_all(&ca_directory).unwrap();
    fs::write(&paths.certificate, [0xff, 0xfe]).unwrap();
    assert_io_context(
        root_ca_status(&ca_directory).unwrap_err(),
        "rsproxy-root-ca.pem",
    );
    assert_io_context(
        read_root_ca(&ca_directory).unwrap_err(),
        "rsproxy-root-ca.pem",
    );
    let _ = fs::remove_dir_all(&ca_directory);

    let ca_directory = temporary_ca_directory("invalid-leaf-bytes");
    let leaf = leaf_paths(&ca_directory, "invalid.example");
    fs::create_dir_all(leaf.certificate.parent().unwrap()).unwrap();
    fs::write(&leaf.certificate, [0xff, 0xfe]).unwrap();
    fs::write(&leaf.private_key, "key").unwrap();
    fs::write(&leaf.chain, "chain").unwrap();
    assert_io_context(
        cached_leaf_certificate(&ca_directory, "invalid.example").unwrap_err(),
        "invalid.example.pem",
    );
    let _ = fs::remove_dir_all(&ca_directory);

    let ca_directory = temporary_ca_directory("leaf-directory-is-file");
    let paths = CaPaths::new(&ca_directory);
    fs::create_dir_all(&ca_directory).unwrap();
    fs::write(&paths.leaf_directory, "blocking file").unwrap();
    assert_io_context(
        store_leaf_certificate(&ca_directory, "host", "cert", "key", "chain").unwrap_err(),
        "create leaf certificate directory",
    );
    let _ = fs::remove_dir_all(&ca_directory);

    for (name, blocked_path) in [
        ("leaf-certificate-is-directory", "certificate"),
        ("leaf-key-is-directory", "key"),
        ("leaf-chain-is-directory", "chain"),
    ] {
        let ca_directory = temporary_ca_directory(name);
        let leaf = leaf_paths(&ca_directory, "host");
        fs::create_dir_all(leaf.certificate.parent().unwrap()).unwrap();
        let path = match blocked_path {
            "certificate" => &leaf.certificate,
            "key" => &leaf.private_key,
            "chain" => &leaf.chain,
            _ => unreachable!(),
        };
        fs::create_dir_all(path).unwrap();
        let error =
            store_leaf_certificate(&ca_directory, "host", "cert", "key", "chain").unwrap_err();
        assert_io_context(error, path.file_name().unwrap().to_str().unwrap());
        let _ = fs::remove_dir_all(ca_directory);
    }
}

#[test]
fn trust_validation_rejects_missing_and_invalid_root_certificates() {
    let options = TrustOptions {
        keychain: Some(temporary_ca_directory("unused-keychain")),
        dry_run: true,
    };
    let missing = temporary_ca_directory("missing-trust-root");
    assert_io_context(
        install_root_ca(&missing, &options).unwrap_err(),
        "rsproxy-root-ca.pem",
    );

    let invalid = temporary_ca_directory("invalid-trust-root");
    let paths = CaPaths::new(&invalid);
    fs::create_dir_all(&invalid).unwrap();
    fs::write(&paths.certificate, "not a certificate").unwrap();
    assert!(matches!(
        uninstall_root_ca(&invalid, &options),
        Err(PlatformError::InvalidState(message)) if message.contains("invalid certificate")
    ));
    let _ = fs::remove_dir_all(invalid);
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
