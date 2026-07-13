use super::support::*;
use super::*;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Frame;
use hyper::service::service_fn;
use hyper::{HeaderMap, Request, Response, Version};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::convert::Infallible;
use std::error::Error as _;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc as std_mpsc;

use rsproxy_net::h2_runtime;

const FIRST_BYTES: usize = 128 * 1024;
const REMAINING_BYTES: usize = 1024 * 1024;

struct OriginObservation {
    bytes: usize,
    trailers: Vec<(String, String)>,
}

fn streaming_state(name: &str, host: &str, origin: std::net::SocketAddr) -> SharedState {
    let rules = format!("{host} host({origin})\n{host} req.body.append(!) when method(POST)");
    let mut state = isolated_state(name, &rules);
    state.config.body_buffer_limit = 32 * 1024;
    state.config.trace_body_limit = 1024;
    state
}

fn spawn_h2_origin(
    listener: TcpListener,
    state: &SharedState,
    tls_host: &str,
) -> (
    tokio::sync::oneshot::Receiver<()>,
    std_mpsc::Receiver<OriginObservation>,
    Arc<AtomicU8>,
    thread::JoinHandle<()>,
) {
    let (cert_path, key_path) = ensure_leaf_certificate(
        &state.config.storage.join("ca"),
        state.config.ca_material.as_ref().unwrap(),
        tls_host,
    )
    .unwrap();
    let mut tls_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            load_certs(&cert_path).unwrap(),
            load_private_key(&key_path).unwrap(),
        )
        .unwrap();
    tls_config.alpn_protocols = vec![H2_ALPN.to_vec()];
    let tls_config = Arc::new(tls_config);
    let (started_sender, started) = tokio::sync::oneshot::channel();
    let started_sender = Arc::new(Mutex::new(Some(started_sender)));
    let (observation_sender, observation) = std_mpsc::channel();
    let (shutdown_sender, shutdown) = tokio::sync::oneshot::channel();
    let shutdown_sender = Arc::new(Mutex::new(Some(shutdown_sender)));
    let stage = Arc::new(AtomicU8::new(0));
    let server_stage = Arc::clone(&stage);
    let server = thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        server_stage.store(1, Ordering::Release);
        let mut tls = StreamOwned::new(ServerConnection::new(tls_config).unwrap(), stream);
        while tls.conn.is_handshaking() {
            tls.conn.complete_io(&mut tls.sock).unwrap();
        }
        assert_eq!(tls.conn.alpn_protocol(), Some(H2_ALPN));
        server_stage.store(2, Ordering::Release);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let service = service_fn(move |request: Request<hyper::body::Incoming>| {
                let started_sender = Arc::clone(&started_sender);
                let observation_sender = observation_sender.clone();
                let shutdown_sender = Arc::clone(&shutdown_sender);
                let request_stage = Arc::clone(&server_stage);
                async move {
                    request_stage.store(3, Ordering::Release);
                    assert_eq!(request.version(), Version::HTTP_2);
                    let mut body = request.into_body();
                    let mut bytes = 0usize;
                    let mut trailers = Vec::new();
                    while let Some(frame) = body.frame().await {
                        let frame = frame.unwrap();
                        match frame.into_data() {
                            Ok(data) => {
                                request_stage.store(4, Ordering::Release);
                                bytes += data.len();
                                if let Some(sender) = started_sender.lock().unwrap().take() {
                                    let _ = sender.send(());
                                }
                            }
                            Err(frame) => {
                                if let Ok(seen) = frame.into_trailers() {
                                    trailers = seen
                                        .iter()
                                        .map(|(name, value)| {
                                            (
                                                name.as_str().to_string(),
                                                value.to_str().unwrap().to_string(),
                                            )
                                        })
                                        .collect();
                                }
                            }
                        }
                    }
                    observation_sender
                        .send(OriginObservation { bytes, trailers })
                        .unwrap();
                    let _ = shutdown_sender.lock().unwrap().take().unwrap().send(());
                    Ok::<_, Infallible>(
                        Response::builder()
                            .status(200)
                            .header("x-origin-protocol", "h2")
                            .body(Full::new(Bytes::from_static(b"ok")))
                            .unwrap(),
                    )
                }
            });
            let connection = hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .serve_connection(
                    TokioIo::new(rsproxy_net::AsyncIo::new(tls).unwrap()),
                    service,
                );
            tokio::pin!(connection);
            let result = tokio::select! {
                result = &mut connection => result,
                _ = shutdown => {
                    connection.as_mut().graceful_shutdown();
                    tokio::time::timeout(Duration::from_secs(3), &mut connection)
                        .await
                        .unwrap()
                }
            };
            if let Err(error) = result {
                assert!(
                    expected_h2_peer_close(&error),
                    "unexpected h2 origin shutdown error: {error:?}"
                );
            }
        });
    });
    (started, observation, stage, server)
}

fn expected_h2_peer_close(error: &hyper::Error) -> bool {
    if error.is_closed() || error.is_canceled() {
        return true;
    }
    let mut cause = error.source();
    while let Some(source) = cause {
        if let Some(io_error) = source.downcast_ref::<std::io::Error>() {
            return matches!(
                io_error.kind(),
                std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::UnexpectedEof
            );
        }
        cause = source.source();
    }
    false
}

#[test]
fn oversized_h1_upload_streams_to_h2_origin_with_trailers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap();
    let host = "h1-to-h2-stream.test";
    let state = streaming_state("h1-to-h2-stream", host, origin);
    let (origin_started, observation, origin_stage, origin_server) =
        spawn_h2_origin(listener, &state, host);
    let (proxy, proxy_server) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);
    connect_request(&mut client, &format!("{host}:443"));
    let mut client = h1_tls_client(client, &state, host);
    while client.conn.is_handshaking() {
        client.conn.complete_io(&mut client.sock).unwrap();
    }
    assert_eq!(client.conn.alpn_protocol(), Some(HTTP1_ALPN));

    write!(
        client,
        "POST /upload HTTP/1.1\r\nHost: {host}\r\nTransfer-Encoding: chunked\r\nTrailer: X-Upload-End\r\nConnection: close\r\n\r\n{:X}\r\n",
        FIRST_BYTES
    )
    .unwrap();
    client.write_all(&vec![b'a'; FIRST_BYTES]).unwrap();
    client.write_all(b"\r\n").unwrap();
    client.flush().unwrap();
    let started = h2_runtime()
        .unwrap()
        .block_on(async { tokio::time::timeout(Duration::from_secs(3), origin_started).await });
    if !matches!(started, Ok(Ok(()))) {
        panic!(
            "origin did not receive the h1 request prefix; stage={} started={started:?} trace={:?}",
            origin_stage.load(Ordering::Acquire),
            state.trace.list(2)
        );
    }

    write!(client, "{:X}\r\n", REMAINING_BYTES).unwrap();
    client.write_all(&vec![b'b'; REMAINING_BYTES]).unwrap();
    client
        .write_all(b"\r\n0\r\nX-Upload-End: h1-done\r\n\r\n")
        .unwrap();
    client.flush().unwrap();
    let response = http::read_response_head(&mut client, 64 * 1024, 128).unwrap();
    assert_eq!(response.status, 200);
    assert_eq!(
        http::header(&response.headers, "x-origin-protocol"),
        Some("h2")
    );
    assert_eq!(
        read_response_body(&mut client, &response.headers)
            .unwrap()
            .body,
        b"ok"
    );

    let observation = observation.recv_timeout(Duration::from_secs(3)).unwrap();
    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    assert_stream_result(&state, observation, "h1-done", false);
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn oversized_h2_upload_streams_to_h2_origin_with_trailers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap();
    let host = "h2-to-h2-stream.test";
    let state = streaming_state("h2-to-h2-stream", host, origin);
    let (origin_started, observation, origin_stage, origin_server) =
        spawn_h2_origin(listener, &state, host);
    let (proxy, proxy_server) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);
    connect_request(&mut client, &format!("{host}:443"));
    let mut client = h2_tls_client(client, &state, host);
    while client.conn.is_handshaking() {
        client.conn.complete_io(&mut client.sock).unwrap();
    }
    assert_eq!(client.conn.alpn_protocol(), Some(H2_ALPN));

    h2_runtime().unwrap().block_on(async {
        let io = TokioIo::new(rsproxy_net::AsyncIo::new(client).unwrap());
        let (mut sender, connection) =
            hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .handshake(io)
                .await
                .unwrap();
        let connection = tokio::spawn(async move {
            let _ = connection.await;
        });
        let (body_sender, body) = channel_request_body(2);
        let request = Request::builder()
            .method("POST")
            .uri(format!("https://{host}/upload"))
            .header("te", "trailers")
            .body(body)
            .unwrap();
        let response = tokio::spawn(async move { sender.send_request(request).await });
        body_sender
            .send(Ok(Frame::data(Bytes::from(vec![b'a'; FIRST_BYTES]))))
            .await
            .unwrap();
        let origin_result = tokio::time::timeout(Duration::from_secs(3), origin_started).await;
        if !matches!(origin_result, Ok(Ok(()))) {
            let response_result = if response.is_finished() {
                Some(response.await)
            } else {
                None
            };
            panic!(
                "origin did not receive the h2 request prefix; stage={} started={origin_result:?} response={response_result:?} trace={:?}",
                origin_stage.load(Ordering::Acquire),
                state.trace.list(2),
            );
        }
        assert!(!response.is_finished());

        body_sender
            .send(Ok(Frame::data(Bytes::from(vec![b'b'; REMAINING_BYTES]))))
            .await
            .unwrap();
        let mut trailers = HeaderMap::new();
        trailers.insert("x-upload-end", "h2-done".parse().unwrap());
        body_sender
            .send(Ok(Frame::trailers(trailers)))
            .await
            .unwrap();
        drop(body_sender);
        let response = tokio::time::timeout(Duration::from_secs(3), response)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["x-origin-protocol"], "h2");
        assert_eq!(
            response.into_body().collect().await.unwrap().to_bytes(),
            Bytes::from_static(b"ok")
        );
        connection.abort();
        let _ = connection.await;
    });

    let observation = observation.recv_timeout(Duration::from_secs(3)).unwrap();
    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    assert_stream_result(&state, observation, "h2-done", true);
    let _ = fs::remove_dir_all(&state.config.storage);
}

fn assert_stream_result(
    state: &SharedState,
    observation: OriginObservation,
    trailer: &str,
    h2_client: bool,
) {
    assert_eq!(observation.bytes, FIRST_BYTES + REMAINING_BYTES);
    assert_eq!(
        observation.trailers,
        vec![("x-upload-end".to_string(), trailer.to_string())]
    );
    let sessions = state.trace.list(2);
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    for flag in [
        "request-streamed",
        "request-body-rewrite-skipped-limit",
        "h2-upstream",
        "h2-upstream-pool-miss",
    ] {
        assert!(session.flags.iter().any(|seen| seen == flag), "{flag}");
    }
    assert_eq!(
        session.flags.iter().any(|seen| seen == "h2-client"),
        h2_client
    );
    assert_eq!(session.request_bytes as usize, observation.bytes);
    assert_eq!(session.req_body_head.len(), 1024);
    assert_eq!(
        http::header(&session.req_trailers, "x-upload-end"),
        Some(trailer)
    );
}
