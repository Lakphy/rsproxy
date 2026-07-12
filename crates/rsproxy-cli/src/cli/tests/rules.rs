use super::super::*;

#[test]
fn rules_request_options_parse_method_headers_and_url() {
    let args = vec![
        "-X".to_string(),
        "OPTIONS".to_string(),
        "-H".to_string(),
        "Origin: https://app.test".to_string(),
        "--header".to_string(),
        "Access-Control-Request-Headers: X-Token".to_string(),
        "--client-ip".to_string(),
        "203.0.113.10".to_string(),
        "--server-ip".to_string(),
        "198.51.100.20".to_string(),
        "--body".to_string(),
        "token=42&mode=beta".to_string(),
        "--response-status".to_string(),
        "202".to_string(),
        "--response-header".to_string(),
        "X-Origin: upstream".to_string(),
        "--api".to_string(),
        "127.0.0.1:8999".to_string(),
        "http://api.test/v1".to_string(),
    ];
    assert_eq!(request_method(&args), "OPTIONS");
    assert_eq!(request_url(&args).as_deref(), Some("http://api.test/v1"));
    assert_eq!(request_client_ip(&args).as_deref(), Some("203.0.113.10"));
    assert_eq!(
        request_server_ip(&args, "http://127.0.0.1/v1").as_deref(),
        Some("198.51.100.20")
    );
    assert_eq!(request_body(&args), b"token=42&mode=beta".to_vec());
    assert_eq!(
        request_headers(&args).unwrap(),
        vec![
            ("Origin".to_string(), "https://app.test".to_string()),
            (
                "Access-Control-Request-Headers".to_string(),
                "X-Token".to_string()
            ),
        ]
    );
    let request = RequestMeta {
        method: request_method(&args),
        url: request_url(&args).unwrap(),
        headers: request_headers(&args).unwrap(),
        body: request_body(&args),
        client_ip: request_client_ip(&args),
        server_ip: request_server_ip(&args, "http://api.test/v1"),
        template: Default::default(),
    };
    let response = response_meta(&args).unwrap().unwrap();
    assert_eq!(
        rules_test_api_path(&request, Some(&response)),
        "/api/rules/test?url=http%3A%2F%2Fapi.test%2Fv1&method=OPTIONS&header=Origin%3A%20https%3A%2F%2Fapp.test&header=Access-Control-Request-Headers%3A%20X-Token&body=token%3D42%26mode%3Dbeta&clientIp=203.0.113.10&serverIp=198.51.100.20&responseStatus=202&responseHeader=X-Origin%3A%20upstream"
    );
}

#[test]
fn rules_request_headers_reject_invalid_syntax() {
    assert!(parse_header_arg("Origin:").is_ok());
    assert!(parse_header_arg(": empty").is_err());
    assert!(parse_header_arg("Origin https://app.test").is_err());
}

#[test]
fn rules_response_options_validate_status_and_default_to_200_for_headers() {
    let headers_only = vec!["--response-header".to_string(), "X-Origin: yes".to_string()];
    let response = response_meta(&headers_only).unwrap().unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers[0],
        ("X-Origin".to_string(), "yes".to_string())
    );

    for value in ["99", "600", "invalid"] {
        let args = vec!["--response-status".to_string(), value.to_string()];
        assert!(response_meta(&args).is_err());
    }
}

#[test]
fn rules_request_url_skips_global_config_values() {
    let args = vec![
        "--config".to_string(),
        "/tmp/rsproxy-config.toml".to_string(),
        "--api-token".to_string(),
        "0123456789abcdef".to_string(),
        "http://api.test/v1".to_string(),
    ];
    assert_eq!(request_url(&args).as_deref(), Some("http://api.test/v1"));
}
