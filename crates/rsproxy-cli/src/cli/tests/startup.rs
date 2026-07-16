use super::*;
use crate::cli::command::StartupCommand;

#[test]
fn startup_install_parses_runtime_and_proxy_selection() {
    let cli = Cli::try_parse_from([
        "rsproxy",
        "startup",
        "install",
        "--storage",
        "/tmp/rsproxy-startup",
        "--config",
        "/tmp/rsproxy.toml",
        "--service",
        "Wi-Fi",
        "--bypass",
        "localhost,*.test",
        "--start-now",
    ])
    .unwrap();
    let Some(TopLevelCommand::Startup(args)) = cli.command else {
        panic!("startup command should parse");
    };
    let StartupCommand::Install(args) = args.command else {
        panic!("startup install should parse");
    };
    assert_eq!(
        args.storage.as_deref(),
        Some(std::path::Path::new("/tmp/rsproxy-startup"))
    );
    assert_eq!(
        args.config.as_deref(),
        Some(std::path::Path::new("/tmp/rsproxy.toml"))
    );
    assert_eq!(args.service.as_deref(), Some("Wi-Fi"));
    assert_eq!(args.bypass.as_deref(), Some("localhost,*.test"));
    assert!(args.start_now);
    assert!(!args.no_system_proxy);
}

#[test]
fn startup_can_disable_automatic_system_proxy() {
    let cli = Cli::try_parse_from([
        "rsproxy",
        "startup",
        "install",
        "--no-system-proxy",
        "--dry-run",
    ])
    .unwrap();
    let Some(TopLevelCommand::Startup(args)) = cli.command else {
        panic!("startup command should parse");
    };
    let StartupCommand::Install(args) = args.command else {
        panic!("startup install should parse");
    };
    assert!(args.no_system_proxy);
    assert!(args.dry_run);
}

#[test]
fn startup_uninstall_defaults_to_safe_runtime_cleanup() {
    let cli = Cli::try_parse_from(["rsproxy", "startup", "uninstall"]).unwrap();
    let Some(TopLevelCommand::Startup(args)) = cli.command else {
        panic!("startup command should parse");
    };
    let StartupCommand::Uninstall(args) = args.command else {
        panic!("startup uninstall should parse");
    };
    assert!(!args.keep_running);
    assert!(!args.dry_run);
}
