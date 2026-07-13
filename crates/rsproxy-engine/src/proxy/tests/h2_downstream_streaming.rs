use super::support::*;
use super::*;
use bytes::Bytes;
use http_body_util::BodyExt;
use hyper::body::Frame;
use hyper::{HeaderMap, Request};
use hyper_util::rt::{TokioExecutor, TokioIo};
use std::sync::mpsc as std_mpsc;

use rsproxy_net::h2_runtime;

const FIRST_REQUEST_BYTES: usize = 128 * 1024;
const REMAINING_REQUEST_BYTES: usize = 1024 * 1024;
const RESPONSE_CHUNK_BYTES: usize = 16 * 1024;
const RESPONSE_CHUNKS: usize = 128;

struct OriginObservation {
    request_bytes: usize,
    request_trailers: Vec<(String, String)>,
}

#[test]
fn downstream_h2_request_and_response_stream_with_bounded_backpressure() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let (request_started_sender, request_started) = tokio::sync::oneshot::channel();
    let (response_started_sender, response_started) = tokio::sync::oneshot::channel();
    let (continue_response, continue_response_receiver) = std_mpsc::channel();
    let (observation_sender, observation_receiver) = std_mpsc::channel();
    let rules = format!(
        "stream.test host({origin_address})\nstream.test req.body.append(!) when method(POST)\nstream.test res.trailer(x-rule-end: yes)"
    );
    let mut state = isolated_state("h2-streaming", &rules);
    state.config.body_buffer_limit = 32 * 1024;
    state.config.trace_body_limit = 1024;
    let (cert_path, key_path) = ensure_leaf_certificate(
        &state.config.storage.join("ca"),
        state.config.ca_material.as_ref().unwrap(),
        "stream.test",
    )
    .unwrap();
    let mut origin_tls = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            load_certs(&cert_path).unwrap(),
            load_private_key(&key_path).unwrap(),
        )
        .unwrap();
    origin_tls.alpn_protocols = vec![HTTP1_ALPN.to_vec()];
    let origin_tls = Arc::new(origin_tls);
    let origin_server = thread::spawn(move || {
        let (upload_stream, _) = origin.accept().unwrap();
        upload_stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let connection = ServerConnection::new(Arc::clone(&origin_tls)).unwrap();
        let mut upload_stream = StreamOwned::new(connection, upload_stream);
        let head = http::read_request_head(&mut upload_stream, 64 * 1024, 128)
            .unwrap()
            .unwrap();
        assert_eq!(head.body, http::RequestBodyFraming::Chunked);
        let mut reader = http::RequestBodyReader::new(head.body);
        let mut buffer = [0u8; 16 * 1024];
        let mut total = 0usize;
        let mut request_started_sender = Some(request_started_sender);
        let trailers = loop {
            match reader
                .read(&mut upload_stream, &mut buffer, 64 * 1024, 128)
                .unwrap()
            {
                http::RequestBodyRead::Data(size) => {
                    total += size;
                    if let Some(sender) = request_started_sender.take() {
                        let _ = sender.send(());
                    }
                }
                http::RequestBodyRead::End(trailers) => break trailers,
            }
        };
        observation_sender
            .send(OriginObservation {
                request_bytes: total,
                request_trailers: trailers,
            })
            .unwrap();
        upload_stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
            .unwrap();
        upload_stream.flush().unwrap();
        drop(upload_stream);

        let (download_stream, _) = origin.accept().unwrap();
        download_stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let connection = ServerConnection::new(origin_tls).unwrap();
        let mut download_stream = StreamOwned::new(connection, download_stream);
        let download_head = http::read_request_head(&mut download_stream, 64 * 1024, 128)
            .unwrap()
            .unwrap();
        assert_eq!(download_head.request.method, "GET");
        assert!(!download_head.body.has_body());
        download_stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nTransfer-Encoding: chunked\r\nTrailer: X-Origin-End\r\n\r\n5\r\nfirst\r\n",
            )
            .unwrap();
        download_stream.flush().unwrap();
        let _ = response_started_sender.send(());
        continue_response_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let chunk = vec![b'r'; RESPONSE_CHUNK_BYTES];
        for _ in 0..RESPONSE_CHUNKS {
            write!(download_stream, "{:X}\r\n", chunk.len()).unwrap();
            download_stream.write_all(&chunk).unwrap();
            download_stream.write_all(b"\r\n").unwrap();
        }
        download_stream
            .write_all(b"0\r\nX-Origin-End: done\r\n\r\n")
            .unwrap();
        download_stream.flush().unwrap();
    });
    let (proxy_address, proxy_server) = spawn_proxy_allowing_h2_disconnect(state.clone(), 1);
    let mut client = connect_client(proxy_address);
    connect_request(&mut client, "stream.test:443");
    let mut tls = h2_tls_client(client, &state, "stream.test");
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock).unwrap();
    }
    assert_eq!(tls.conn.alpn_protocol(), Some(H2_ALPN));

    let runtime = h2_runtime().unwrap();
    runtime.block_on(async {
        let io = TokioIo::new(rsproxy_net::AsyncIo::new(tls).unwrap());
        let builder = hyper::client::conn::http2::Builder::new(TokioExecutor::new());
        let (mut sender, connection) = builder.handshake(io).await.unwrap();
        let connection_task = tokio::spawn(connection);
        let (body_sender, body) = channel_request_body(2);
        let request = Request::builder()
            .method("POST")
            .uri("https://stream.test/upload")
            .header(
                "content-length",
                (FIRST_REQUEST_BYTES + REMAINING_REQUEST_BYTES).to_string(),
            )
            .header("te", "trailers")
            .body(body)
            .unwrap();
        let response_task = tokio::spawn(async move {
            let response = sender.send_request(request).await;
            (sender, response)
        });

        body_sender
            .send(Ok(Frame::data(Bytes::from(vec![
                b'a';
                FIRST_REQUEST_BYTES
            ]))))
            .await
            .unwrap();
        if tokio::time::timeout(Duration::from_secs(3), request_started)
            .await
            .ok()
            .and_then(Result::ok)
            .is_none()
        {
            panic!(
                "origin did not receive the request prefix; response_finished={} trace={:?}",
                response_task.is_finished(),
                state.trace.list(10)
            );
        }
        assert!(!response_task.is_finished());

        body_sender
            .send(Ok(Frame::data(Bytes::from(vec![
                b'b';
                REMAINING_REQUEST_BYTES
            ]))))
            .await
            .unwrap();
        let mut request_trailers = HeaderMap::new();
        request_trailers.insert("x-upload-end", "done".parse().unwrap());
        body_sender
            .send(Ok(Frame::trailers(request_trailers)))
            .await
            .unwrap();
        drop(body_sender);

        let (mut sender, upload_response) =
            tokio::time::timeout(Duration::from_secs(3), response_task)
                .await
                .expect("upload response did not complete")
                .unwrap();
        let upload_response = upload_response.unwrap();
        assert_eq!(upload_response.status(), 200);
        assert_eq!(
            upload_response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes(),
            Bytes::from_static(b"ok")
        );

        let (empty_sender, empty_body) = channel_request_body(1);
        drop(empty_sender);
        let download_request = Request::builder()
            .method("GET")
            .uri("https://stream.test/download")
            .body(empty_body)
            .unwrap();
        let mut response_task =
            tokio::spawn(async move { sender.send_request(download_request).await });
        if tokio::time::timeout(Duration::from_secs(3), response_started)
            .await
            .ok()
            .and_then(Result::ok)
            .is_none()
        {
            let response = if response_task.is_finished() {
                Some((&mut response_task).await)
            } else {
                None
            };
            panic!(
                "origin did not start response; response={response:?} trace={:?}",
                state.trace.list(10)
            );
        }
        let response = tokio::time::timeout(Duration::from_secs(3), response_task)
            .await
            .expect("response headers were aggregated behind the complete response body")
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(!response.headers().contains_key("content-length"));
        assert!(!response.headers().contains_key("transfer-encoding"));
        let mut body = response.into_body();
        let first = tokio::time::timeout(Duration::from_secs(3), body.frame())
            .await
            .expect("first response DATA frame was not streamed")
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert_eq!(first, Bytes::from_static(b"first"));
        continue_response.send(()).unwrap();

        let mut response_bytes = first.len();
        let mut response_trailers = HeaderMap::new();
        while let Some(frame) = body.frame().await {
            let frame = frame.unwrap();
            match frame.into_data() {
                Ok(data) => response_bytes += data.len(),
                Err(frame) => {
                    if let Ok(trailers) = frame.into_trailers() {
                        response_trailers = trailers;
                    }
                }
            }
        }
        assert_eq!(
            response_bytes,
            b"first".len() + RESPONSE_CHUNK_BYTES * RESPONSE_CHUNKS
        );
        assert_eq!(response_trailers["x-origin-end"], "done");
        assert_eq!(response_trailers["x-rule-end"], "yes");
        tokio::time::timeout(Duration::from_secs(3), connection_task)
            .await
            .expect("h2 client connection should close within the shutdown deadline")
            .expect("h2 client connection task should not panic")
            .expect("h2 client connection should shut down cleanly after GOAWAY");
    });

    let observation = observation_receiver
        .recv_timeout(Duration::from_secs(3))
        .unwrap();
    assert_eq!(
        observation.request_bytes,
        FIRST_REQUEST_BYTES + REMAINING_REQUEST_BYTES
    );
    assert_eq!(
        observation.request_trailers,
        vec![("x-upload-end".to_string(), "done".to_string())]
    );
    origin_server.join().unwrap();
    proxy_server.join().unwrap();

    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 2);
    let upload = sessions
        .iter()
        .find(|session| session.url.ends_with("/upload"))
        .unwrap();
    for flag in [
        "h2-client",
        "mitm",
        "request-streamed",
        "request-body-rewrite-skipped-limit",
    ] {
        assert!(upload.flags.iter().any(|seen| seen == flag), "{flag}");
    }
    assert_eq!(upload.request_bytes as usize, observation.request_bytes);
    assert_eq!(upload.req_body_head.len(), 1024);
    let download = sessions
        .iter()
        .find(|session| session.url.ends_with("/download"))
        .unwrap();
    for flag in ["h2-client", "mitm", "response-streamed"] {
        assert!(download.flags.iter().any(|seen| seen == flag), "{flag}");
    }
    assert_eq!(download.res_body_head.len(), 1024);
    assert_eq!(
        http::header(&download.res_trailers, "x-origin-end"),
        Some("done")
    );
    assert_eq!(
        http::header(&download.res_trailers, "x-rule-end"),
        Some("yes")
    );
    let _ = fs::remove_dir_all(&state.config.storage);
}
