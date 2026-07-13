use super::*;
use std::net::Ipv6Addr;
use std::sync::mpsc;

#[test]
fn ipv6_literal_and_punycode_host_route_over_real_network_paths() {
    let ipv6_origin = TcpListener::bind((Ipv6Addr::LOCALHOST, 0)).unwrap();
    let ipv6_address = ipv6_origin.local_addr().unwrap();
    let (ipv6_seen, ipv6_server) = spawn_origin(ipv6_origin, b"ipv6-ok");
    let punycode_origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let punycode_address = punycode_origin.local_addr().unwrap();
    let (punycode_seen, punycode_server) = spawn_origin(punycode_origin, b"punycode-ok");
    let state = isolated_state(
        "protocol-names",
        &format!("xn--bcher-kva.test host({punycode_address})"),
    );
    let (proxy, proxy_server) = spawn_proxy(state.clone(), 2);

    let ipv6_authority = ipv6_address.to_string();
    let (status, body) = plain_request(
        proxy,
        &format!("http://{ipv6_authority}/ipv6"),
        &ipv6_authority,
    );
    assert_eq!(
        status,
        200,
        "IPv6 proxy response: {}; trace={:?}",
        String::from_utf8_lossy(&body),
        state.trace.list(4)
    );
    assert_eq!(body, b"ipv6-ok");

    let punycode_host = "xn--bcher-kva.test";
    let (status, body) = plain_request(
        proxy,
        &format!("http://{punycode_host}/punycode"),
        punycode_host,
    );
    assert_eq!(status, 200);
    assert_eq!(body, b"punycode-ok");

    assert_eq!(
        ipv6_seen.recv_timeout(Duration::from_secs(2)).unwrap(),
        ("/ipv6".to_string(), ipv6_authority.clone())
    );
    assert_eq!(
        punycode_seen.recv_timeout(Duration::from_secs(2)).unwrap(),
        ("/punycode".to_string(), punycode_host.to_string())
    );
    ipv6_server.join().unwrap();
    punycode_server.join().unwrap();
    proxy_server.join().unwrap();

    let sessions = state.trace.list(4);
    assert_eq!(sessions.len(), 2);
    assert!(
        sessions
            .iter()
            .any(|session| session.url == format!("http://{ipv6_authority}/ipv6"))
    );
    assert!(sessions.iter().any(|session| {
        session.url == format!("http://{punycode_host}/punycode")
            && session.upstream.as_deref() == Some(&punycode_address.to_string())
    }));
    let _ = fs::remove_dir_all(&state.config.storage);
}

fn spawn_origin(
    listener: TcpListener,
    body: &'static [u8],
) -> (mpsc::Receiver<(String, String)>, thread::JoinHandle<()>) {
    let (sender, receiver) = mpsc::channel();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let request = http::read_request_head(&mut stream, 64 * 1024, 128)
            .unwrap()
            .unwrap();
        sender
            .send((
                request.request.target,
                http::header(&request.request.headers, "host")
                    .unwrap()
                    .to_string(),
            ))
            .unwrap();
        write!(
            stream,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )
        .unwrap();
        stream.write_all(body).unwrap();
        stream.flush().unwrap();
    });
    (receiver, server)
}

fn plain_request(proxy: std::net::SocketAddr, url: &str, host: &str) -> (u16, Vec<u8>) {
    let mut client = connect_client(proxy);
    write!(
        client,
        "GET {url} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    client.flush().unwrap();
    let response = http::read_response_head(&mut client, 64 * 1024, 128).unwrap();
    let body = read_response_body(&mut client, &response.headers)
        .unwrap()
        .body;
    (response.status, body)
}
