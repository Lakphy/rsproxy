use crate::cli::command::{Cli, RulesCommand, TopLevelCommand};
use crate::cli::rules::request::request_meta;
use crate::cli::rules::{parse_header_arg, response_meta, rules_test_api_path};
use clap::Parser;

#[test]
fn rules_request_options_are_typed_and_preserve_multi_value_order() {
    let cli = Cli::try_parse_from([
        "rsproxy",
        "rules",
        "test",
        "-X",
        "OPTIONS",
        "-H",
        "Origin: https://app.test",
        "--header",
        "Access-Control-Request-Headers: X-Token",
        "-H",
        "Origin: https://app.test",
        "--client-ip",
        "203.0.113.10",
        "--server-ip",
        "198.51.100.20",
        "--body",
        "token=42&mode=beta",
        "--response-status",
        "202",
        "--response-header",
        "X-Origin: upstream",
        "--response-header",
        "X-Origin: upstream",
        "--api",
        "127.0.0.1:8999",
        "http://api.test/v1",
    ])
    .unwrap();
    let Some(TopLevelCommand::Rules(rules)) = cli.command else {
        panic!("rules command expected");
    };
    let RulesCommand::Test(args) = rules.command else {
        panic!("rules test command expected");
    };

    assert_eq!(args.request.method, "OPTIONS");
    assert_eq!(
        args.request.header,
        [
            "Origin: https://app.test",
            "Access-Control-Request-Headers: X-Token",
            "Origin: https://app.test",
        ]
    );
    assert_eq!(
        args.response_header,
        ["X-Origin: upstream", "X-Origin: upstream"]
    );
    let response = response_meta(args.response_status.as_deref(), &args.response_header)
        .unwrap()
        .unwrap();
    let request = request_meta(&args.request, args.url).unwrap();
    assert_eq!(request.client_ip.as_deref(), Some("203.0.113.10"));
    assert_eq!(request.server_ip.as_deref(), Some("198.51.100.20"));
    assert_eq!(request.body, b"token=42&mode=beta".to_vec());
    assert_eq!(
        rules_test_api_path(&request, Some(&response)),
        "/api/rules/test?url=http%3A%2F%2Fapi.test%2Fv1&method=OPTIONS&header=Origin%3A%20https%3A%2F%2Fapp.test&header=Access-Control-Request-Headers%3A%20X-Token&header=Origin%3A%20https%3A%2F%2Fapp.test&body=token%3D42%26mode%3Dbeta&clientIp=203.0.113.10&serverIp=198.51.100.20&responseStatus=202&responseHeader=X-Origin%3A%20upstream&responseHeader=X-Origin%3A%20upstream"
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
    let headers = vec!["X-Origin: yes".to_string()];
    let response = response_meta(None, &headers).unwrap().unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers[0],
        ("X-Origin".to_string(), "yes".to_string())
    );

    for value in ["99", "600", "invalid"] {
        assert!(response_meta(Some(value), &[]).is_err());
    }
}

#[test]
fn rules_request_url_is_not_confused_with_global_config_values() {
    let cli = Cli::try_parse_from([
        "rsproxy",
        "rules",
        "test",
        "--config",
        "/tmp/rsproxy-config.toml",
        "--api-token",
        "0123456789abcdef",
        "http://api.test/v1",
    ])
    .unwrap();
    let Some(TopLevelCommand::Rules(rules)) = cli.command else {
        panic!("rules command expected");
    };
    let RulesCommand::Test(args) = rules.command else {
        panic!("rules test command expected");
    };
    assert_eq!(args.url, "http://api.test/v1");
}
