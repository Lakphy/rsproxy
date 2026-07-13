use super::{CliError, ConfigError, DaemonConflict};
use std::path::PathBuf;

#[test]
fn error_codes_and_exit_codes_follow_the_cli_contract() {
    let usage = CliError::Usage("bad option".to_string());
    assert_eq!(usage.code(), "usage_error");
    assert_eq!(usage.exit_code(), 2);

    let conflict = CliError::DaemonConflict(DaemonConflict::NotRunning {
        pid_path: PathBuf::from("run/rsproxy.pid"),
    });
    assert_eq!(conflict.code(), "daemon_conflict");
    assert_eq!(conflict.exit_code(), 3);

    let config = CliError::Config(ConfigError::Invalid("zero timeout".to_string()));
    assert_eq!(config.code(), "config_error");
    assert_eq!(config.exit_code(), 1);
}
