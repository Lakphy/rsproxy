use super::error_message;
use rsproxy_cli::CliError;
use rsproxy_control::ControlError;

#[test]
fn human_http_errors_drop_the_json_transport_wrapper() {
    let error = CliError::Control(ControlError::HttpStatus {
        status: 502,
        body: r#"{"error":"origin refused the replay"}"#.to_string(),
    });
    assert_eq!(error_message(&error), "origin refused the replay");
}
