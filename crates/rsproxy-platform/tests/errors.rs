use rsproxy_platform::PlatformError;
use std::error::Error as _;
use std::io;

#[test]
fn io_error_preserves_its_source() {
    let error = PlatformError::Io {
        context: "read trust store".to_string(),
        source: io::Error::new(io::ErrorKind::PermissionDenied, "access denied"),
    };

    let source = error.source().expect("I/O source should be retained");
    assert_eq!(source.to_string(), "access denied");
}

#[test]
fn certificate_conversion_retains_rcgen_error_type() {
    let error = PlatformError::from(rcgen::Error::CouldNotParseCertificate);
    let source = error
        .source()
        .expect("certificate source should be retained");
    assert!(source.is::<rcgen::Error>());
}

#[test]
fn command_failure_renders_only_sanitized_output() {
    let error = PlatformError::CommandFailed {
        command: "security".to_string(),
        status: Some(1),
        output: "certificate was not trusted".to_string(),
    };

    assert!(error.to_string().contains("certificate was not trusted"));
}
