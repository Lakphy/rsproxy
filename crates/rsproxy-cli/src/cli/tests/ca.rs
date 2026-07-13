use crate::cli::ca::{
    ca_export, ca_init, ca_issue, ca_status, display_ca_command_args, print_trust_outcome,
    validate_leaf_host,
};
use crate::cli::command::{
    CaCommand, CaExportArgs, CaInitArgs, CaIssueArgs, CaStatusArgs, Cli, TopLevelCommand,
};
use clap::Parser;
use rsproxy_platform::ca::{TrustAction, TrustCommand, TrustOutcome};
use std::path::PathBuf;

#[test]
fn ca_issue_host_is_a_typed_positional() {
    let cli = Cli::try_parse_from([
        "rsproxy",
        "ca",
        "issue",
        "--config",
        "/tmp/rsproxy.toml",
        "--storage",
        "/tmp/rsproxy",
        "--force",
        "api.example.test",
    ])
    .unwrap();
    let Some(TopLevelCommand::Ca(args)) = cli.command else {
        panic!("ca command expected");
    };
    let Some(CaCommand::Issue(args)) = args.command else {
        panic!("ca issue command expected");
    };
    assert_eq!(args.host, "api.example.test");
    assert!(args.force);
}

#[test]
fn ca_keychain_argument_is_typed_and_requires_a_value() {
    let cli = Cli::try_parse_from([
        "rsproxy",
        "ca",
        "status",
        "--keychain",
        "/tmp/login.keychain-db",
    ])
    .unwrap();
    let Some(TopLevelCommand::Ca(args)) = cli.command else {
        panic!("ca command expected");
    };
    let Some(CaCommand::Status(args)) = args.command else {
        panic!("ca status command expected");
    };
    assert_eq!(args.keychain, Some(PathBuf::from("/tmp/login.keychain-db")));
    assert!(Cli::try_parse_from(["rsproxy", "ca", "status", "--keychain"]).is_err());
    assert!(Cli::try_parse_from(["rsproxy", "ca", "status", "--keychain", "--json"]).is_err());
}

#[test]
fn ca_trust_plan_renderer_quotes_whitespace_without_changing_argument_order() {
    let args = vec![
        "add-trusted-cert".to_string(),
        "Contract Keychain.keychain-db".to_string(),
    ];
    assert_eq!(
        display_ca_command_args(&args),
        "add-trusted-cert \"Contract Keychain.keychain-db\""
    );
    assert_eq!(
        display_ca_command_args(&[String::new(), "plain".to_string()]),
        "\"\" plain"
    );
}

#[test]
fn ca_local_lifecycle_covers_status_export_issue_cache_and_validation() {
    let root = std::env::temp_dir().join(format!(
        "rsproxy-cli-ca-unit-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let ca_directory = root.join("ca");

    ca_status(
        CaStatusArgs {
            keychain: Some(root.join("unused.keychain")),
        },
        &ca_directory,
        false,
    )
    .unwrap();
    ca_init(
        CaInitArgs {
            force: false,
            name: None,
        },
        &ca_directory,
    )
    .unwrap();
    ca_init(
        CaInitArgs {
            force: false,
            name: Some("ignored because the CA exists".to_string()),
        },
        &ca_directory,
    )
    .unwrap();
    ca_status(CaStatusArgs { keychain: None }, &ca_directory, false).unwrap();

    let exported = root.join("exported.pem");
    ca_export(
        CaExportArgs {
            output: Some(exported.clone()),
        },
        &ca_directory,
    )
    .unwrap();
    assert!(
        std::fs::read_to_string(&exported)
            .unwrap()
            .contains("BEGIN CERTIFICATE")
    );
    ca_export(CaExportArgs { output: None }, &ca_directory).unwrap();

    for host in ["", "bad host", "bad/host"] {
        assert!(validate_leaf_host(host).is_err(), "{host:?}");
    }
    validate_leaf_host("api.example.test").unwrap();
    ca_issue(
        CaIssueArgs {
            host: "api.example.test".to_string(),
            force: false,
        },
        &ca_directory,
    )
    .unwrap();
    ca_issue(
        CaIssueArgs {
            host: "api.example.test".to_string(),
            force: false,
        },
        &ca_directory,
    )
    .unwrap();
    ca_issue(
        CaIssueArgs {
            host: "api.example.test".to_string(),
            force: true,
        },
        &ca_directory,
    )
    .unwrap();

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn trust_outcome_renderer_covers_completed_platform_contracts() {
    let mut dry_run = trust_outcome("linux", TrustAction::Install);
    dry_run.dry_run = true;
    dry_run.commands = vec![TrustCommand {
        program: "trust-tool".to_string(),
        args: vec![String::new(), "path with spaces".to_string()],
    }];
    print_trust_outcome(false, &dry_run).unwrap();
    print_trust_outcome(true, &dry_run).unwrap();

    let mut macos_install = trust_outcome("macos", TrustAction::Install);
    macos_install.keychain = Some(PathBuf::from("login.keychain-db"));
    print_trust_outcome(false, &macos_install).unwrap();
    let mut macos_uninstall = trust_outcome("macos", TrustAction::Uninstall);
    macos_uninstall.keychain = Some(PathBuf::from("login.keychain-db"));
    macos_uninstall.trust_settings_removed = Some(true);
    macos_uninstall.removed_certificate = Some(false);
    macos_uninstall.installed = Some(false);
    print_trust_outcome(false, &macos_uninstall).unwrap();

    let mut windows = trust_outcome("windows", TrustAction::Install);
    windows.thumbprint_sha1 = Some("AA11".to_string());
    print_trust_outcome(false, &windows).unwrap();
    print_trust_outcome(true, &windows).unwrap();

    let linux = trust_outcome("linux", TrustAction::Uninstall);
    print_trust_outcome(false, &linux).unwrap();
    print_trust_outcome(true, &linux).unwrap();

    assert!(print_trust_outcome(false, &trust_outcome("macos", TrustAction::Install)).is_err());
    assert!(print_trust_outcome(false, &trust_outcome("windows", TrustAction::Install)).is_err());
}

fn trust_outcome(platform: &'static str, action: TrustAction) -> TrustOutcome {
    TrustOutcome {
        platform,
        backend: "test-backend",
        action,
        certificate: PathBuf::from("root.pem"),
        fingerprint_sha256: "AA:BB".to_string(),
        keychain: None,
        thumbprint_sha1: None,
        dry_run: false,
        commands: Vec::new(),
        trust_settings_removed: None,
        removed_certificate: None,
        installed: None,
    }
}
