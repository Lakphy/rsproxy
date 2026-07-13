use super::ControlError;
use rsproxy_engine::EngineError;
use std::error::Error as _;
use std::io;

#[test]
fn http_status_display_is_only_the_response_body() {
    let error = ControlError::HttpStatus {
        status: 422,
        body: "rule validation failed".to_string(),
    };

    assert_eq!(error.to_string(), "rule validation failed");
    assert!(!error.to_string().contains("422"));
}

#[test]
fn engine_conversion_preserves_the_source_chain() {
    let error = ControlError::from(EngineError::InvalidInput("empty host".to_string()));
    let source = error.source().expect("engine source should be retained");
    assert!(source.is::<EngineError>());
}

#[test]
fn io_error_preserves_context_and_source() {
    let error = ControlError::io(
        "read control response",
        io::Error::new(io::ErrorKind::UnexpectedEof, "truncated"),
    );

    assert_eq!(error.to_string(), "read control response: truncated");
    let source = error.source().expect("I/O source should be retained");
    assert!(source.is::<io::Error>());
}

#[test]
fn json_error_preserves_context_and_source() {
    let source =
        serde_json::from_str::<serde_json::Value>("{").expect_err("truncated JSON should fail");
    let error = ControlError::Json {
        context: "decode control response".to_string(),
        source,
    };

    assert!(error.to_string().starts_with("decode control response:"));
    let source = error.source().expect("JSON source should be retained");
    assert!(source.is::<serde_json::Error>());
}

#[test]
fn authentication_display_preserves_validation_text() {
    let error = ControlError::Authentication("--api-token is invalid".to_string());
    assert_eq!(error.to_string(), "--api-token is invalid");
}
