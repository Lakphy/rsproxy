use super::*;
use crate as http;
use crate::RequestBodyFraming;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{HeaderMap, Method, Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::io;
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc as std_mpsc;
use std::thread;
use std::time::Duration;
use tokio::runtime::Builder as RuntimeBuilder;

fn is_expected_h2_disconnect(error: &io::Error) -> bool {
    let message = format!("{error:?}");
    message.contains("hyper::Error(Io")
        && [
            "NotConnected",
            "ConnectionReset",
            "BrokenPipe",
            "UnexpectedEof",
        ]
        .iter()
        .any(|kind| message.contains(kind))
}

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
    let (sender, receiver) = mpsc::channel(4);
    sender
        .try_send(Ok(DownstreamH2ResponseFrame::Data(Bytes::from_static(
            b"hello",
        ))))
        .unwrap();
    sender
        .try_send(Ok(DownstreamH2ResponseFrame::Trailers(vec![(
            "X-Checksum".to_string(),
            "abc".to_string(),
        )])))
        .unwrap();
    drop(sender);
    let response = hyper_response(DownstreamH2Response {
        head: DownstreamH2ResponseHead {
            status: 207,
            headers: vec![
                ("Content-Type".to_string(), "text/plain".to_string()),
                ("Connection".to_string(), "close".to_string()),
                ("Transfer-Encoding".to_string(), "chunked".to_string()),
            ],
        },
        body: receiver,
    })
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

#[test]
fn h2_request_trailer_validation_uses_downstream_config() {
    let config = DownstreamH2Config {
        max_header_size: 128,
        max_header_count: 1,
    };
    let mut too_many = HeaderMap::new();
    too_many.insert("x-one", "1".parse().unwrap());
    too_many.insert("x-two", "2".parse().unwrap());
    let error = message::request_trailers(&too_many, &config).unwrap_err();
    assert_eq!(error.status, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
    assert!(error.message.contains("trailer count limit exceeded"));

    let mut forbidden = HeaderMap::new();
    forbidden.insert("content-length", "1".parse().unwrap());
    let error = message::request_trailers(&forbidden, &config).unwrap_err();
    assert_eq!(error.status, StatusCode::BAD_REQUEST);
    assert!(error.message.contains("forbidden request trailer"));
}

#[test]
fn downstream_h2_server_delegates_streams_through_callback() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let (observed_sender, observed_receiver) = std_mpsc::channel();
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let result = serve_downstream_h2(
            stream,
            "fallback.test:443".to_string(),
            DownstreamH2Config {
                max_header_size: 16 * 1024,
                max_header_count: 64,
            },
            move |mut request| {
                let observed_sender = observed_sender.clone();
                async move {
                    if request.head.request.target == "/handler-error" {
                        return Err(io::Error::other("callback failed"));
                    }
                    let method = request.head.request.method.clone();
                    let target = request.head.request.target.clone();
                    let authority = request.authority.clone();
                    let framing = request.head.body;
                    let mut body = Vec::new();
                    let mut trailers = Vec::new();
                    while let Some(frame) = request.body.recv().await {
                        match frame? {
                            DownstreamH2RequestFrame::Data(data) => body.extend_from_slice(&data),
                            DownstreamH2RequestFrame::Trailers(seen) => trailers = seen,
                        }
                    }
                    observed_sender
                        .send((method, target, authority, framing, body, trailers))
                        .unwrap();

                    let (body_sender, response_body) = mpsc::channel(2);
                    body_sender
                        .send(Ok(DownstreamH2ResponseFrame::Data(Bytes::from_static(
                            b"callback-response",
                        ))))
                        .await
                        .unwrap();
                    body_sender
                        .send(Ok(DownstreamH2ResponseFrame::Trailers(vec![(
                            "x-callback-end".to_string(),
                            "done".to_string(),
                        )])))
                        .await
                        .unwrap();
                    drop(body_sender);
                    Ok(DownstreamH2Response {
                        head: DownstreamH2ResponseHead {
                            status: 202,
                            headers: vec![(
                                "content-type".to_string(),
                                "application/octet-stream".to_string(),
                            )],
                        },
                        body: response_body,
                    })
                }
            },
        );
        match result {
            Ok(()) => {}
            Err(error) if is_expected_h2_disconnect(&error) => {}
            Err(error) => panic!("unexpected downstream h2 server error: {error:?}"),
        }
    });

    let runtime = RuntimeBuilder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let stream = TcpStream::connect(address).unwrap();
        let io = TokioIo::new(crate::AsyncIo::new(stream).unwrap());
        let (mut sender, connection) =
            hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .handshake(io)
                .await
                .unwrap();
        let connection = tokio::spawn(connection);
        let response = sender
            .send_request(
                Request::builder()
                    .method("POST")
                    .uri("https://callback.test/items?q=1")
                    .header("x-callback", "yes")
                    .body(Full::new(Bytes::from_static(b"callback-request")))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 202);
        assert_eq!(
            response.headers()["content-type"],
            "application/octet-stream"
        );
        let collected = response.into_body().collect().await.unwrap();
        assert_eq!(collected.trailers().unwrap()["x-callback-end"], "done");
        assert_eq!(
            collected.to_bytes(),
            Bytes::from_static(b"callback-response")
        );

        let error_response = sender
            .send_request(
                Request::builder()
                    .uri("https://callback.test/handler-error")
                    .body(Full::new(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(error_response.status(), 502);
        let error_body = error_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert!(String::from_utf8_lossy(&error_body).contains("callback failed"));

        let oversized_response = sender
            .send_request(
                Request::builder()
                    .uri("https://callback.test/oversized")
                    .header("x-oversized", "x".repeat(17 * 1024))
                    .body(Full::new(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            oversized_response.status(),
            StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE
        );
        let oversized_body = oversized_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert!(String::from_utf8_lossy(&oversized_body).contains("header size limit exceeded"));

        let connect_response = sender
            .send_request(
                Request::builder()
                    .method(Method::CONNECT)
                    .uri("callback.test:443")
                    .body(Full::new(Bytes::new()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(connect_response.status(), StatusCode::NOT_IMPLEMENTED);
        let connect_body = connect_response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes();
        assert!(String::from_utf8_lossy(&connect_body).contains("CONNECT over HTTP/2"));

        drop(sender);
        tokio::time::timeout(Duration::from_secs(3), connection)
            .await
            .expect("h2 client connection should close within the shutdown deadline")
            .expect("h2 client connection task should not panic")
            .expect("h2 client connection should shut down cleanly after GOAWAY");
    });

    let observed = observed_receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert_eq!(observed.0, "POST");
    assert_eq!(observed.1, "/items?q=1");
    assert_eq!(observed.2, "callback.test");
    assert_eq!(observed.3, RequestBodyFraming::Chunked);
    assert_eq!(observed.4, b"callback-request");
    assert!(observed.5.is_empty());
    server.join().unwrap();
}
