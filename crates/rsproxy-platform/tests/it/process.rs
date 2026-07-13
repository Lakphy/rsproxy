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

#[cfg(unix)]
#[test]
fn unix_process_helpers_cover_detach_invalid_and_absent_processes() {
    let mut command = std::process::Command::new("/bin/sh");
    command.args(["-c", "exit 0"]);
    detach_daemon(&mut command);
    assert!(command.status().unwrap().success());

    assert!(process_alive(std::process::id()));
    assert!(!process_alive(u32::MAX));
    assert!(matches!(
        terminate_process(u32::MAX),
        Err(PlatformError::InvalidState(_))
    ));
    assert!(matches!(
        force_terminate_process(u32::MAX),
        Err(PlatformError::InvalidState(_))
    ));

    let absent_pid = i32::MAX as u32;
    assert!(!process_alive(absent_pid));
    terminate_process(absent_pid).unwrap();
    force_terminate_process(absent_pid).unwrap();
    assert_eq!(resident_kib(absent_pid), None);
    assert!(resident_kib(std::process::id()).is_some());
}
