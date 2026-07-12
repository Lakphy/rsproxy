use super::*;
use hyper::header::HeaderValue;
use hyper::{HeaderMap, Version};

#[test]
fn request_conversion_strips_h1_only_headers_and_invalid_te() {
    let request = UpstreamH2Request {
        method: "POST".to_string(),
        uri: "https://example.test/items".to_string(),
        headers: vec![
            ("Host".to_string(), "example.test".to_string()),
            ("Connection".to_string(), "close, x-remove".to_string()),
            ("X-Remove".to_string(), "internal".to_string()),
            ("Transfer-Encoding".to_string(), "chunked".to_string()),
            ("TE".to_string(), "gzip".to_string()),
            ("X-Test".to_string(), "yes".to_string()),
        ],
        body: b"body".to_vec(),
        trailers: Vec::new(),
    };

    let request = hyper_request(request, 4096, 16).unwrap();

    assert_eq!(request.version(), Version::HTTP_2);
    assert_eq!(request.uri(), "https://example.test/items");
    assert_eq!(request.headers().get("x-test").unwrap(), "yes");
    assert!(!request.headers().contains_key("host"));
    assert!(!request.headers().contains_key("connection"));
    assert!(!request.headers().contains_key("x-remove"));
    assert!(!request.headers().contains_key("transfer-encoding"));
    assert!(!request.headers().contains_key("te"));
}

#[test]
fn trailers_only_te_is_preserved() {
    let mut headers = HeaderMap::new();
    append_request_headers(
        &mut headers,
        vec![("TE".to_string(), "trailers".to_string())],
    )
    .unwrap();

    assert_eq!(headers.get("te").unwrap(), "trailers");
}

#[test]
fn response_header_count_limit_includes_status_pseudo_header() {
    let mut headers = HeaderMap::new();
    headers.insert("x-one", HeaderValue::from_static("1"));

    let error = validate_header_limits(&headers, 4096, 1, 1, 35, "response").unwrap_err();

    assert!(error.to_string().contains("header count limit exceeded"));
}
