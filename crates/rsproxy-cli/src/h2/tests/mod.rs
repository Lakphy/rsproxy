use super::*;
use crate::http;
use crate::proxy::{H2BridgeFrame, H2BridgeHead};
use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::Request;
use tokio::runtime::Builder as RuntimeBuilder;

#[test]
fn h2_request_conversion_maps_pseudo_headers_and_strips_connection_headers() {
    let request = Request::builder()
        .method("POST")
        .uri("https://example.test:8443/items?q=1")
        .header("connection", "close")
        .header("proxy-authorization", "Basic secret")
        .header("x-test", "yes")
        .body(())
        .unwrap();
    let (parts, _) = request.into_parts();

    let (request, authority) = raw_request(
        parts,
        b"body".to_vec(),
        vec![("x-checksum".to_string(), "abc".to_string())],
        "fallback.test:443",
    )
    .expect("HTTP/2 request should convert");

    assert_eq!(authority, "example.test:8443");
    assert_eq!(request.target, "/items?q=1");
    assert_eq!(request.version, "HTTP/2");
    assert_eq!(
        http::header(&request.headers, "host"),
        Some("example.test:8443")
    );
    assert_eq!(http::header(&request.headers, "x-test"), Some("yes"));
    assert_eq!(http::header(&request.headers, "content-length"), Some("4"));
    assert!(http::header(&request.headers, "connection").is_none());
    assert!(http::header(&request.headers, "proxy-authorization").is_none());
    assert_eq!(
        request.trailers,
        vec![("x-checksum".to_string(), "abc".to_string())]
    );
}

#[test]
fn h2_response_conversion_preserves_body_and_trailers() {
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    sender
        .try_send(Ok(H2BridgeFrame::Data(Bytes::from_static(b"hello"))))
        .unwrap();
    sender
        .try_send(Ok(H2BridgeFrame::Trailers(vec![(
            "X-Checksum".to_string(),
            "abc".to_string(),
        )])))
        .unwrap();
    drop(sender);
    let response = hyper_response(
        H2BridgeHead {
            status: 207,
            headers: vec![
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("Connection".to_string(), "close".to_string()),
                ("Transfer-Encoding".to_string(), "chunked".to_string()),
            ],
        },
        receiver,
    )
    .expect("bridged response should be valid");

    assert_eq!(response.status(), 207);
    assert_eq!(response.headers()["content-type"], "text/plain");
    assert!(!response.headers().contains_key("connection"));
    assert!(!response.headers().contains_key("transfer-encoding"));
    let runtime = RuntimeBuilder::new_current_thread().build().unwrap();
    let collected = runtime
        .block_on(response.into_body().collect())
        .expect("body collection should succeed");
    assert_eq!(collected.trailers().unwrap()["x-checksum"], "abc");
    assert_eq!(collected.to_bytes(), Bytes::from_static(b"hello"));
}
