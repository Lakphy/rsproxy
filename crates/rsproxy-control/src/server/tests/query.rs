use super::super::*;

#[test]
fn query_get_all_preserves_repeated_headers() {
    let query = Some(
        "url=http%3A%2F%2Fapi.test&header=Origin%3A%20https%3A%2F%2Fapp.test&header=Access-Control-Request-Headers%3A%20X-Token&clientIp=203.0.113.10&serverIp=198.51.100.20",
    );
    assert_eq!(query_get(query, "url").as_deref(), Some("http://api.test"));
    assert_eq!(
        query_get(query, "clientIp").as_deref(),
        Some("203.0.113.10")
    );
    assert_eq!(
        query_get(query, "serverIp").as_deref(),
        Some("198.51.100.20")
    );
    assert_eq!(
        query_get_all(query, "header"),
        vec![
            "Origin: https://app.test".to_string(),
            "Access-Control-Request-Headers: X-Token".to_string()
        ]
    );
}

#[test]
fn parse_header_query_value_trims_value_and_rejects_empty_name() {
    assert_eq!(
        parse_header_query_value("Origin: https://app.test"),
        Some(("Origin".to_string(), "https://app.test".to_string()))
    );
    assert_eq!(parse_header_query_value(": missing"), None);
    assert_eq!(parse_header_query_value("Origin https://app.test"), None);
}
