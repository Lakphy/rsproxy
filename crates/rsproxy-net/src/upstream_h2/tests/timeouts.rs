use super::*;
use crate::upstream_body::UpstreamBodyFrame;
use bytes::Bytes;
use http_body::{Body, Frame};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::future::Future as _;
use std::net::TcpListener;
use std::pin::Pin;
use std::sync::mpsc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::runtime::Builder as RuntimeBuilder;

struct DelayedBody {
    delay: Pin<Box<tokio::time::Sleep>>,
    data: Option<Bytes>,
}

impl Body for DelayedBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.data.is_none() {
            return Poll::Ready(None);
        }
        if self.delay.as_mut().poll(cx).is_pending() {
            return Poll::Pending;
        }
        Poll::Ready(self.data.take().map(Frame::data).map(Ok))
    }
}

#[test]
fn ttfb_and_request_total_timeouts_have_independent_scopes() {
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
                if path == "/slow-head" {
                    tokio::time::sleep(Duration::from_millis(80)).await;
                }
                let body: BoxBody<Bytes, Infallible> = if path == "/slow-body" {
                    DelayedBody {
                        delay: Box::pin(tokio::time::sleep(Duration::from_millis(250))),
                        data: Some(Bytes::from_static(b"slow")),
                    }
                    .boxed()
                } else {
                    Full::new(Bytes::from_static(b"fast")).boxed()
                };
                Ok::<_, Infallible>(Response::new(body))
            });
            let result = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service)
                .await;
            let _ = done_tx.send(result);
        });
    });

    let pool_key = format!("test-h2-timeouts-{addr}");
    let request = |path: &str| UpstreamH2Request {
        method: "GET".to_string(),
        uri: format!("https://example.test{path}"),
        headers: Vec::new(),
        body: Vec::new(),
        trailers: Vec::new(),
    };
    let connector = expect_connector(
        dispatch_buffered(
            &pool_key,
            request("/slow-head"),
            test_config(
                8,
                Duration::from_secs(1),
                Duration::from_millis(40),
                request_deadline(),
            ),
        )
        .unwrap(),
    );
    let error = connect_response(connector, TcpStream::connect(addr).unwrap()).unwrap_err();
    assert_eq!(error.to_string(), "upstream_h2 ttfb: timeout after 40ms");

    let slow_body = expect_response(
        dispatch_buffered(
            &pool_key,
            request("/slow-body"),
            test_config(
                8,
                Duration::from_secs(1),
                Duration::from_secs(1),
                request_deadline(),
            ),
        )
        .unwrap(),
    );
    assert!(slow_body.ttfb_ms < 250);
    let mut response_body = slow_body.body;
    let mut received = Vec::new();
    while let Some(frame) = response_body.next() {
        if let UpstreamBodyFrame::Data(data) = frame.unwrap() {
            received.extend_from_slice(&data);
        }
    }
    assert_eq!(received, b"slow");
    assert!(response_body.receive_ms().unwrap() >= 200);

    let total_response = expect_response(
        dispatch_buffered(
            &pool_key,
            request("/slow-body"),
            test_config(
                8,
                Duration::from_secs(1),
                Duration::from_secs(1),
                RequestDeadline::new(Duration::from_millis(40)).unwrap(),
            ),
        )
        .unwrap(),
    );
    let total_error = total_response.body.collect().unwrap_err();
    assert_eq!(
        total_error.to_string(),
        "stage=request_total: timeout after 40ms"
    );
    assert!(h2_pool().inner.lock().unwrap().get(&pool_key).is_some());

    let fast = expect_response(
        dispatch_buffered(
            &pool_key,
            request("/fast"),
            test_config(
                8,
                Duration::from_secs(1),
                Duration::from_secs(1),
                request_deadline(),
            ),
        )
        .unwrap(),
    );
    assert_eq!(fast.body.collect().unwrap().body, b"fast");

    let generation = h2_pool()
        .inner
        .lock()
        .unwrap()
        .entries
        .get(&pool_key)
        .unwrap()
        .generation;
    remove_pool_entry(&pool_key, generation);
    assert!(
        done_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap()
            .is_ok()
    );
}
