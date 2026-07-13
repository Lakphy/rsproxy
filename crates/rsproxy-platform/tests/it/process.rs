use rsproxy_platform::PlatformError;
use rsproxy_platform::process::*;

#[test]
fn pid_parser_rejects_invalid_and_reserved_values() {
    assert_eq!(parse_pid(" 42\n").unwrap(), 42);
    assert!(matches!(
        parse_pid("not-a-pid"),
        Err(PlatformError::InvalidState(_))
    ));
    assert!(matches!(
        parse_pid("0"),
        Err(PlatformError::InvalidState(_))
    ));
    assert!(matches!(
        parse_pid("1"),
        Err(PlatformError::InvalidState(_))
    ));
}

#[cfg(unix)]
#[test]
fn control_socket_path_hashes_only_when_unix_path_is_too_long() {
    let short = std::path::Path::new("/tmp/rsproxy-test");
    assert_eq!(unix_control_socket_path(short), short.join("run/ctl.sock"));

    let long = std::path::PathBuf::from(format!("/tmp/{}", "x".repeat(120)));
    let first = unix_control_socket_path(&long);
    assert!(first.starts_with("/tmp"));
    assert_eq!(first, unix_control_socket_path(&long));
    assert!(first.to_string_lossy().len() <= 96);
}
