use super::*;

fn spawn_scripted_proxy(
    script: impl FnOnce(&mut TcpStream) + Send + 'static,
) -> (String, u16, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let worker = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        script(&mut stream);
    });
    (address.ip().to_string(), address.port(), worker)
}

fn read_http_head(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0];
    while !bytes.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 4096);
    }
    String::from_utf8(bytes).unwrap()
}

fn accept_http_connect(stream: &mut TcpStream, expected_target: &str) {
    let request = read_http_head(stream);
    assert!(
        request.starts_with(&format!("CONNECT {expected_target} HTTP/1.1\r\n")),
        "unexpected CONNECT request: {request:?}"
    );
    stream
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .unwrap();
}

fn read_socks_target(stream: &mut TcpStream) -> (String, u16) {
    let mut head = [0; 4];
    stream.read_exact(&mut head).unwrap();
    assert_eq!(&head[..3], &[0x05, 0x01, 0x00]);
    let host = match head[3] {
        0x01 => {
            let mut octets = [0; 4];
            stream.read_exact(&mut octets).unwrap();
            std::net::Ipv4Addr::from(octets).to_string()
        }
        0x03 => {
            let mut len = [0];
            stream.read_exact(&mut len).unwrap();
            let mut host = vec![0; len[0] as usize];
            stream.read_exact(&mut host).unwrap();
            String::from_utf8(host).unwrap()
        }
        0x04 => {
            let mut octets = [0; 16];
            stream.read_exact(&mut octets).unwrap();
            std::net::Ipv6Addr::from(octets).to_string()
        }
        atyp => panic!("unexpected SOCKS address type: {atyp:#x}"),
    };
    let mut port = [0; 2];
    stream.read_exact(&mut port).unwrap();
    (host, u16::from_be_bytes(port))
}

fn accept_socks_connect(
    stream: &mut TcpStream,
    expected_target: (&str, u16),
    credentials: Option<(&str, &str)>,
) {
    let mut greeting = [0; 2];
    stream.read_exact(&mut greeting).unwrap();
    assert_eq!(greeting[0], 0x05);
    let mut methods = vec![0; greeting[1] as usize];
    stream.read_exact(&mut methods).unwrap();
    if let Some((username, password)) = credentials {
        assert!(methods.contains(&0x02));
        stream.write_all(&[0x05, 0x02]).unwrap();
        let mut auth_head = [0; 2];
        stream.read_exact(&mut auth_head).unwrap();
        assert_eq!(auth_head[0], 0x01);
        let mut seen_username = vec![0; auth_head[1] as usize];
        stream.read_exact(&mut seen_username).unwrap();
        let mut password_len = [0];
        stream.read_exact(&mut password_len).unwrap();
        let mut seen_password = vec![0; password_len[0] as usize];
        stream.read_exact(&mut seen_password).unwrap();
        assert_eq!(seen_username, username.as_bytes());
        assert_eq!(seen_password, password.as_bytes());
        stream.write_all(&[0x01, 0x00]).unwrap();
    } else {
        assert!(methods.contains(&0x00));
        stream.write_all(&[0x05, 0x00]).unwrap();
    }
    let target = read_socks_target(stream);
    assert_eq!(target, (expected_target.0.to_string(), expected_target.1));
    stream
        .write_all(&[0x05, 0x00, 0x00, 0x01, 127, 0, 0, 1, 0, 0])
        .unwrap();
}

fn assert_echo(mut upstream: UpstreamStream) {
    upstream.write_all(b"ping").unwrap();
    upstream.flush().unwrap();
    let mut response = [0; 4];
    upstream.read_exact(&mut response).unwrap();
    assert_eq!(&response, b"pong");
}

fn finish_echo(stream: &mut TcpStream) {
    let mut request = [0; 4];
    stream.read_exact(&mut request).unwrap();
    assert_eq!(&request, b"ping");
    stream.write_all(b"pong").unwrap();
}

fn connect_route(route: &UpstreamRoute) -> io::Result<UpstreamStream> {
    connect_tunnel_upstream_recorded(
        route,
        &test_state(),
        &mut Vec::new(),
        &mut NetworkTimings::default(),
        request_deadline(),
    )
}

#[test]
fn socks_and_http_tunnel_routes_complete_their_handshakes() {
    let (host, port, worker) = spawn_scripted_proxy(|stream| {
        accept_socks_connect(stream, ("origin.test", 443), Some(("alice", "secret")));
        finish_echo(stream);
    });
    let route = UpstreamRoute::Socks5 {
        proxy_host: host,
        proxy_port: port,
        auth: Some(SocksAuth {
            username: "alice".to_string(),
            password: "secret".to_string(),
        }),
        target_host: "origin.test".to_string(),
        target_port: 443,
    };
    assert_echo(connect_route(&route).unwrap());
    worker.join().unwrap();

    let (host, port, worker) = spawn_scripted_proxy(|stream| {
        accept_http_connect(stream, "origin.test:8443");
        finish_echo(stream);
    });
    let route = UpstreamRoute::HttpProxy {
        proxy_host: host,
        proxy_port: port,
        target_host: "origin.test".to_string(),
        target_port: 8443,
    };
    assert_echo(connect_route(&route).unwrap());
    worker.join().unwrap();
}

#[test]
fn proxy_chain_transitions_between_http_and_socks_hops() {
    let (host, port, worker) = spawn_scripted_proxy(|stream| {
        accept_http_connect(stream, "socks.internal:1080");
        accept_socks_connect(stream, ("origin.test", 443), None);
        finish_echo(stream);
    });
    let route = UpstreamRoute::ProxyChain {
        hops: vec![
            ProxyHop::Http { host, port },
            ProxyHop::Socks5 {
                host: "socks.internal".to_string(),
                port: 1080,
                auth: None,
            },
        ],
        target_host: "origin.test".to_string(),
        target_port: 443,
    };
    assert_echo(connect_route(&route).unwrap());
    worker.join().unwrap();

    let (host, port, worker) = spawn_scripted_proxy(|stream| {
        accept_socks_connect(stream, ("http.internal", 8080), None);
        accept_http_connect(stream, "origin.test:80");
        finish_echo(stream);
    });
    let route = UpstreamRoute::ProxyChain {
        hops: vec![
            ProxyHop::Socks5 {
                host,
                port,
                auth: None,
            },
            ProxyHop::Http {
                host: "http.internal".to_string(),
                port: 8080,
            },
        ],
        target_host: "origin.test".to_string(),
        target_port: 80,
    };
    assert_echo(connect_route(&route).unwrap());
    worker.join().unwrap();
}

#[test]
fn empty_proxy_chain_is_rejected_before_network_access() {
    let route = UpstreamRoute::ProxyChain {
        hops: Vec::new(),
        target_host: "origin.test".to_string(),
        target_port: 443,
    };
    let error = match connect_route(&route) {
        Ok(_) => panic!("empty proxy chain unexpectedly connected"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(error.to_string(), "proxy chain is empty");

    let error = match connect_proxy_chain_to_final(
        &[],
        &test_state(),
        &mut Vec::new(),
        &mut NetworkTimings::default(),
        request_deadline(),
    ) {
        Ok(_) => panic!("empty proxy chain unexpectedly connected"),
        Err(error) => error,
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}
