use super::*;

#[test]
fn security_command_runner_maps_success_failure_and_spawn_errors() {
    let mut success = Command::new("/bin/sh");
    success.args(["-c", "printf 'ok'"]);
    let output = security_output("success", &mut success).unwrap();
    assert_eq!(output.stdout, b"ok");

    let mut failure = Command::new("/bin/sh");
    failure.args(["-c", "printf 'denied' >&2; exit 7"]);
    let error = security_output("failure", &mut failure).unwrap_err();
    match error {
        PlatformError::CommandFailed {
            command,
            status,
            output,
        } => {
            assert_eq!(command, "failure");
            assert_eq!(status, Some(7));
            assert_eq!(output, "denied");
        }
        other => panic!("expected typed command failure, got {other:?}"),
    }

    let mut missing = Command::new("/path/that/does/not/exist/rsproxy-security-test");
    let error = security_raw_output("missing", &mut missing).unwrap_err();
    match error {
        PlatformError::Io { context, source } => {
            assert_eq!(context, "missing");
            assert_eq!(source.kind(), std::io::ErrorKind::NotFound);
        }
        other => panic!("expected typed spawn failure, got {other:?}"),
    }
}

#[test]
fn security_output_helpers_classify_messages() {
    fn output(script: &str) -> std::process::Output {
        Command::new("/bin/sh")
            .args(["-c", script])
            .output()
            .unwrap()
    }

    let both = output("printf 'out'; printf 'err' >&2; exit 1");
    assert_eq!(security_output_message(&both), "err; out");
    assert!(!security_output_is_not_found(&both));

    let stderr = output("printf 'No matching item' >&2; exit 1");
    assert_eq!(security_output_message(&stderr), "No matching item");
    assert!(security_output_is_not_found(&stderr));

    let stdout = output("printf 'certificate could not be found'; exit 1");
    assert_eq!(
        security_output_message(&stdout),
        "certificate could not be found"
    );
    assert!(security_output_is_not_found(&stdout));

    let empty = output("exit 9");
    assert!(security_output_message(&empty).contains("exit status: 9"));
}
