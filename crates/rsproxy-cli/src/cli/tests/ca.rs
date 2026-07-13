use crate::cli::ca::display_ca_command_args;
use crate::cli::command::{CaCommand, Cli, TopLevelCommand};
use clap::Parser;
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
}
