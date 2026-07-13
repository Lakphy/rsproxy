use super::*;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::net::TcpListener;
use std::sync::mpsc;
use tokio::runtime::Builder as RuntimeBuilder;

const FIRST_BYTES: usize = 128 * 1024;
const REMAINING_BYTES: usize = 1024 * 1024;

struct Observation {
    path: String,
    bytes: usize,
    trailer: Option<String>,
}

#[test]
fn pooled_streaming_request_sends_prefix_before_completion_and_preserves_trailer() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let (started_sender, started) = mpsc::channel();
    let (observation_sender, observation) = mpsc::channel();
    let (done_sender, done) = mpsc::channel();
    std::thread::spawn(move || {
        let runtime = RuntimeBuilder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).unwrap();
            let (stream, _) = listener.accept().await.unwrap();
            let service = service_fn(move |request: Request<Incoming>| {
                let started_sender = started_sender.clone();
                let observation_sender = observation_sender.clone();
                async move {
                    let path = request.uri().path().to_string();
                    let mut body = request.into_body();
                    let mut bytes = 0usize;
                    let mut trailer = None;
                    while let Some(frame) = body.frame().await {
                        let frame = frame.unwrap();
                        match frame.into_data() {
                            Ok(data) => {
                                bytes += data.len();
                                if path == "/stream" && bytes == data.len() {
                                    let _ = started_sender.send(());
                                }
                            }
                            Err(frame) => {
                                if let Ok(trailers) = frame.into_trailers() {
                                    trailer = trailers
                                        .get("x-upload-end")
                                        .and_then(|value| value.to_str().ok())
                                        .map(str::to_string);
                                }
                            }
                        }
                    }
                    observation_sender
                        .send(Observation {
                            path,
                            bytes,
                            trailer,
                        })
                        .unwrap();
                    Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ok"))))
                }
            });
            let result = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .serve_connection(TokioIo::new(stream), service)
                .await;
            let _ = done_sender.send(result);
        });
    });

    let pool_key = format!("streaming-pool-{address}");
    let request = |path: &str| UpstreamH2Request {
        method: "POST".to_string(),
        uri: format!("https://stream.test{path}"),
        headers: vec![("TE".to_string(), "trailers".to_string())],
        body: Vec::new(),
        trailers: Vec::new(),
    };
    let connector = expect_connector(
        dispatch_buffered(
            &pool_key,
            request("/seed"),
            test_config(
                4,
                Duration::from_secs(1),
                Duration::from_secs(1),
                request_deadline(),
            ),
        )
        .unwrap(),
    );
    let seed = connect_response(connector, TcpStream::connect(address).unwrap()).unwrap();
    assert_eq!(seed.body.collect().unwrap().body, b"ok");

    let mut stream = match dispatch(H2DispatchRequest {
        pool_key: &pool_key,
        request: request("/stream"),
        body: H2Body::Streaming,
        config: test_config(
            4,
            Duration::from_secs(1),
            Duration::from_secs(1),
            request_deadline(),
        ),
    })
    .unwrap()
    {
        H2Outcome::Streaming(stream) => stream,
        H2Outcome::Connect(_) | H2Outcome::Response(_) => {
            panic!("seed request should leave a pooled h2 connection")
        }
    };
    assert!(
        stream
            .send_data(Bytes::from(vec![b'a'; FIRST_BYTES]), request_deadline())
            .unwrap()
    );
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    std::thread::sleep(Duration::from_millis(60));
    assert!(
        stream
            .send_data(Bytes::from(vec![b'b'; REMAINING_BYTES]), request_deadline(),)
            .unwrap()
    );
    assert!(
        stream
            .send_trailers(
                vec![("x-upload-end".to_string(), "done".to_string())],
                request_deadline(),
            )
            .unwrap()
    );
    stream.close_body();
    let response = stream
        .finish(Duration::from_secs(1), request_deadline())
        .unwrap();
    assert!(response.reused_connection);
    assert!(response.request_send_ms >= 50);
    assert_eq!(response.body.collect().unwrap().body, b"ok");

    let observations = [
        observation.recv_timeout(Duration::from_secs(2)).unwrap(),
        observation.recv_timeout(Duration::from_secs(2)).unwrap(),
    ];
    let streamed = observations
        .iter()
        .find(|seen| seen.path == "/stream")
        .unwrap();
    assert_eq!(streamed.bytes, FIRST_BYTES + REMAINING_BYTES);
    assert_eq!(streamed.trailer.as_deref(), Some("done"));

    let generation = h2_pool()
        .inner
        .lock()
        .unwrap()
        .get(&pool_key)
        .unwrap()
        .generation;
    remove_pool_entry(&pool_key, generation);
    let _ = done.recv_timeout(Duration::from_secs(2));
}
