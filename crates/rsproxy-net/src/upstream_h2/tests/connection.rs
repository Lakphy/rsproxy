use super::*;
use bytes::Bytes;
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::header::HeaderValue;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tokio::runtime::Builder as RuntimeBuilder;

#[test]
fn pooled_connection_preserves_grpc_body_headers_and_trailers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    std::thread::spawn(move || {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(|request: Request<Incoming>| async move {
                let path = request.uri().path().to_string();
                let collected = request.into_body().collect().await.unwrap();
                let request_trailer = collected
                    .trailers()
                    .and_then(|trailers| trailers.get("x-request-checksum"))
                    .and_then(|value| value.to_str().ok())
                    .unwrap_or("missing")
                    .to_string();
                let body = collected.to_bytes();
                let mut trailers = HeaderMap::new();
                trailers.insert("grpc-status", HeaderValue::from_static("0"));
                trailers.insert("grpc-message", HeaderValue::from_static("ok"));
                let response_body: BoxBody<Bytes, Infallible> = Full::new(body)
                    .with_trailers(async move { Some(Ok(trailers)) })
                    .boxed();
                Ok::<_, Infallible>(
                    Response::builder()
                        .status(200)
                        .header("content-type", "application/grpc")
                        .header("x-origin-path", path)
                        .header("x-request-trailer", request_trailer)
                        .body(response_body)
                        .unwrap(),
                )
            });
            let result = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service)
                .await;
            let _ = done_tx.send(result);
        });
    });

    let pool_key = format!("test-grpc-{addr}");
    let request = |path: &str| UpstreamH2Request {
        method: "POST".to_string(),
        uri: format!("https://example.test{path}"),
        headers: vec![
            ("Content-Type".to_string(), "application/grpc".to_string()),
            ("TE".to_string(), "trailers".to_string()),
        ],
        body: vec![0, 0, 0, 0, 0],
        trailers: vec![("x-request-checksum".to_string(), "abc".to_string())],
    };
    let first_connector = expect_connector(
        dispatch_buffered(
            &pool_key,
            request("/echo.First"),
            test_config(
                256,
                Duration::from_secs(1),
                Duration::from_secs(1),
                request_deadline(),
            ),
        )
        .unwrap(),
    );
    let first = connect_response(first_connector, TcpStream::connect(addr).unwrap()).unwrap();
    assert!(!first.reused_connection);
    assert_eq!(first.status, 200);
    assert_eq!(
        first
            .headers
            .iter()
            .find(|(name, _)| name == "x-request-trailer")
            .map(|(_, value)| value.as_str()),
        Some("abc")
    );
    let first_body = first.body.collect().unwrap();
    assert_eq!(first_body.body, vec![0, 0, 0, 0, 0]);
    assert_eq!(
        first_body
            .trailers
            .iter()
            .find(|(name, _)| name == "grpc-status")
            .map(|(_, value)| value.as_str()),
        Some("0")
    );

    let second = expect_response(
        dispatch_buffered(
            &pool_key,
            request("/echo.Second"),
            test_config(
                256,
                Duration::from_secs(1),
                Duration::from_secs(1),
                request_deadline(),
            ),
        )
        .unwrap(),
    );
    assert!(second.reused_connection);
    assert_eq!(
        second
            .headers
            .iter()
            .find(|(name, _)| name == "x-origin-path")
            .map(|(_, value)| value.as_str()),
        Some("/echo.Second")
    );
    let second_body = second.body.collect().unwrap();
    assert_eq!(
        second_body
            .trailers
            .iter()
            .find(|(name, _)| name == "grpc-message")
            .map(|(_, value)| value.as_str()),
        Some("ok")
    );

    h2_pool()
        .inner
        .lock()
        .unwrap()
        .entries
        .get_mut(&pool_key)
        .unwrap()
        .last_used = Instant::now() - H2_POOL_IDLE_TTL;
    assert!(matches!(
        dispatch_buffered(
            &pool_key,
            request("/echo.Expired"),
            test_config(
                256,
                Duration::from_secs(1),
                Duration::from_secs(1),
                request_deadline(),
            ),
        )
        .unwrap(),
        H2Outcome::Connect(_)
    ));
    let _ = done_rx.recv_timeout(Duration::from_secs(2));
}
