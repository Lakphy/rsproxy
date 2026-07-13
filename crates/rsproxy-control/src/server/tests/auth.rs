use super::super::*;

#[test]
fn control_token_auth_accepts_supported_headers_and_rejects_malformed_values() {
    let expected = "0123456789abcdef0123456789abcdef";
    assert!(control_authorized(&[], None));
    assert!(control_authorized(
        &[("Authorization".to_string(), format!("bearer  {expected}"))],
        Some(expected)
    ));
    assert!(control_authorized(
        &[("X-Rsproxy-Token".to_string(), format!(" {expected} "))],
        Some(expected)
    ));

    for value in [
        "Basic MDEyMzQ1Njc4OWFiY2RlZg==",
        "Bearer wrong-token-value",
        "Bearer 0123456789abcdef extra",
    ] {
        assert!(!control_authorized(
            &[("Authorization".to_string(), value.to_string())],
            Some(expected)
        ));
    }
    assert!(!control_authorized(&[], Some(expected)));
}

#[test]
fn unauthorized_control_response_is_bearer_challenge() {
    let mut output = Vec::new();
    respond_control_unauthorized(&mut output).unwrap();
    let response = String::from_utf8(output).unwrap();
    assert!(response.starts_with("HTTP/1.1 401 Unauthorized\r\n"));
    assert!(response.contains("WWW-Authenticate: Bearer\r\n"));
    assert!(response.ends_with("{\"error\":\"unauthorized\"}"));
}
