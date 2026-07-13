use super::*;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::{Request, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};

use rsproxy_net::h2_runtime;

const ALLOWED_HEADER_BYTES: usize = 200 * 1024;
const HEADER_LIMIT: usize = 256 * 1024;

#[test]
fn h1_large_header_accepts_200kb_and_rejects_over_limit_with_431() {
    let mut state = isolated_state("protocol-h1-headers", "headers.matrix.test status(209)");
    state.config.max_header_size = HEADER_LIMIT;
    let (proxy, proxy_server) = spawn_proxy(state.clone(), 2);

    let (status, _) = h1_request_with_header(proxy, ALLOWED_HEADER_BYTES);
    assert_eq!(status, 209);
    let (status, body) = h1_request_with_header(proxy, HEADER_LIMIT + 1024);
    assert_eq!(status, 431);
    let body = String::from_utf8(body).unwrap();
    assert!(body.contains("header size limit exceeded"));

    proxy_server.join().unwrap();
    let sessions = state.trace.list(4);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, Some(209));
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn h2_large_header_accepts_200kb_and_rejects_over_limit_with_431() {
    let mut state = isolated_state("protocol-h2-headers", "headers.matrix.test status(209)");
    state.config.max_header_size = HEADER_LIMIT;
    let (proxy, proxy_server) = spawn_proxy_allowing_h2_disconnect(state.clone(), 1);
    let mut client = connect_client(proxy);
    connect_request(&mut client, "headers.matrix.test:443");
    let mut client = h2_tls_client(client, &state, "headers.matrix.test");
    while client.conn.is_handshaking() {
        client.conn.complete_io(&mut client.sock).unwrap();
    }

    h2_runtime().unwrap().block_on(async {
        let io = TokioIo::new(rsproxy_net::AsyncIo::new(client).unwrap());
        let (mut sender, connection) =
            hyper::client::conn::http2::Builder::new(TokioExecutor::new())
                .handshake(io)
                .await
                .unwrap();
        let connection = tokio::spawn(connection);

        let allowed = h2_request(&mut sender, "/allowed", ALLOWED_HEADER_BYTES).await;
        assert_eq!(allowed.0, StatusCode::from_u16(209).unwrap());
        let rejected = h2_request(&mut sender, "/rejected", HEADER_LIMIT + 1024).await;
        assert_eq!(rejected.0, StatusCode::REQUEST_HEADER_FIELDS_TOO_LARGE);
        let body = String::from_utf8(rejected.1.to_vec()).unwrap();
        assert!(body.contains("header size limit exceeded"));
        assert!(body.contains(&HEADER_LIMIT.to_string()));

        drop(sender);
        tokio::time::timeout(Duration::from_secs(3), connection)
            .await
            .expect("h2 client connection should close within the shutdown deadline")
            .expect("h2 client connection task should not panic")
            .expect("h2 client connection should shut down cleanly after GOAWAY");
    });

    proxy_server.join().unwrap();
    let sessions = state.trace.list(4);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].status, Some(209));
    let _ = fs::remove_dir_all(&state.config.storage);
}

fn h1_request_with_header(proxy: std::net::SocketAddr, size: usize) -> (u16, Vec<u8>) {
    let mut client = connect_client(proxy);
    write!(
        client,
        "GET http://headers.matrix.test/ HTTP/1.1\r\nHost: headers.matrix.test\r\nX-Large: {}\r\nConnection: close\r\n\r\n",
        "a".repeat(size)
    )
    .unwrap();
    client.flush().unwrap();
    let response = http::read_response_head(&mut client, 64 * 1024, 128).unwrap();
    let body = read_response_body(&mut client, &response.headers)
        .unwrap()
        .body;
    (response.status, body)
}

async fn h2_request(
    sender: &mut hyper::client::conn::http2::SendRequest<Full<Bytes>>,
    path: &str,
    size: usize,
) -> (StatusCode, Bytes) {
    let value = hyper::header::HeaderValue::from_bytes(&vec![b'a'; size]).unwrap();
    let request = Request::builder()
        .uri(format!("https://headers.matrix.test{path}"))
        .header("x-large", value)
        .body(Full::new(Bytes::new()))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, body)
}
