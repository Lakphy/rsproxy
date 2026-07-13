use super::support::*;
use super::*;

const TLS_HOST: &str = "tunnel-origin.test";

fn tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let connector = thread::spawn(move || TcpStream::connect(address).unwrap());
    let (accepted, _) = listener.accept().unwrap();
    let connected = connector.join().unwrap();
    for stream in [&accepted, &connected] {
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
    }
    (accepted, connected)
}

fn tunnel_trace() -> (rsproxy_trace::TraceStore, TunnelTrace, u64) {
    let store = rsproxy_trace::TraceStore::new(4);
    let id = store.start(rsproxy_trace::SessionStart {
        kind: SessionKind::Tunnel,
        started_ms: rsproxy_trace::now_millis(),
        method: "CONNECT".to_string(),
        url: TLS_HOST.to_string(),
        client: "127.0.0.1:1".to_string(),
    });
    (store.clone(), TunnelTrace::new(store, id).unwrap(), id)
}

fn finish_trace(store: &rsproxy_trace::TraceStore, id: u64) -> Session {
    store.emit(rsproxy_trace::TraceEvent::End {
        id,
        kind: SessionKind::Tunnel,
        duration_ms: 1,
        pool_wait_ms: 0,
        dns_ms: 0,
        connect_ms: 0,
        ttfb_ms: 0,
        request_send_ms: None,
        response_receive_ms: None,
        upstream: Some(TLS_HOST.to_string()),
        flags: vec!["tunnel".to_string()],
        error: None,
    });
    store.list(1).pop().unwrap()
}

#[test]
fn tcp_tunnel_copies_both_half_closed_directions_and_traces_counts() {
    let (tunnel_client, mut downstream) = tcp_pair();
    let (tunnel_upstream, mut origin) = tcp_pair();
    let (store, trace, id) = tunnel_trace();

    let origin_worker = thread::spawn(move || {
        let mut request = Vec::new();
        origin.read_to_end(&mut request).unwrap();
        assert_eq!(request, b"request-through-tunnel");
        origin.write_all(b"response-through-tunnel").unwrap();
        origin.shutdown(Shutdown::Write).unwrap();
    });
    let tunnel_worker = thread::spawn(move || {
        tunnel_copy(
            tunnel_client,
            UpstreamStream::Tcp(tunnel_upstream),
            Some(trace),
        )
    });

    downstream.write_all(b"request-through-tunnel").unwrap();
    downstream.shutdown(Shutdown::Write).unwrap();
    let mut response = Vec::new();
    downstream.read_to_end(&mut response).unwrap();
    assert_eq!(response, b"response-through-tunnel");

    origin_worker.join().unwrap();
    let counts = tunnel_worker.join().unwrap();
    assert_eq!(counts, (22, 23));
    let session = finish_trace(&store, id);
    assert_eq!(session.request_bytes, 22);
    assert_eq!(session.response_bytes, 23);
}

#[test]
fn tls_tunnel_drives_handshake_and_application_data_in_both_directions() {
    let state = isolated_state("tls-tunnel-copy", "");
    let (cert_path, key_path) = ensure_leaf_certificate(
        &state.config.storage.join("ca"),
        state.config.ca_material.as_ref().unwrap(),
        TLS_HOST,
    )
    .unwrap();
    let server_config = Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(
                load_certs(&cert_path).unwrap(),
                load_private_key(&key_path).unwrap(),
            )
            .unwrap(),
    );
    let mut roots = RootCertStore::empty();
    for certificate in
        load_certs_from_pem(state.config.ca_material.as_ref().unwrap().certificate_pem()).unwrap()
    {
        roots.add(certificate).unwrap();
    }
    let client_config = Arc::new(
        ClientConfig::builder()
            .with_root_certificates(roots)
            .with_no_client_auth(),
    );

    let (tunnel_client, mut downstream) = tcp_pair();
    let (proxy_socket, origin_socket) = tcp_pair();
    let client_connection = ClientConnection::new(
        client_config,
        ServerName::try_from(TLS_HOST.to_string()).unwrap(),
    )
    .unwrap();
    let upstream = StreamOwned::new(client_connection, UpstreamStream::Tcp(proxy_socket));
    let (store, trace, id) = tunnel_trace();

    let origin_worker = thread::spawn(move || {
        let connection = ServerConnection::new(server_config).unwrap();
        let mut tls = StreamOwned::new(connection, origin_socket);
        let mut request = [0; 17];
        tls.read_exact(&mut request).unwrap();
        assert_eq!(&request, b"encrypted request");
        tls.write_all(b"encrypted response").unwrap();
        tls.conn.send_close_notify();
        tls.flush().unwrap();
    });
    let tunnel_worker =
        thread::spawn(move || tunnel_copy_tls(tunnel_client, upstream, Some(trace)));

    downstream.write_all(b"encrypted request").unwrap();
    downstream.flush().unwrap();
    let mut response = Vec::new();
    downstream.read_to_end(&mut response).unwrap();
    assert_eq!(response, b"encrypted response");
    let _ = downstream.shutdown(Shutdown::Write);

    origin_worker.join().unwrap();
    let counts = tunnel_worker.join().unwrap();
    assert_eq!(counts, (17, 18));
    let session = finish_trace(&store, id);
    assert_eq!(session.request_bytes, 17);
    assert_eq!(session.response_bytes, 18);
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn tunnel_helpers_classify_terminal_errors_and_flush_pending_bytes() {
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::TimedOut] {
        assert!(would_block(&io::Error::new(kind, "retry")));
    }
    assert!(!would_block(&io::Error::new(
        io::ErrorKind::Interrupted,
        "retry elsewhere",
    )));
    for kind in [
        io::ErrorKind::UnexpectedEof,
        io::ErrorKind::ConnectionReset,
        io::ErrorKind::ConnectionAborted,
        io::ErrorKind::BrokenPipe,
    ] {
        assert!(tunnel_end_error(&io::Error::new(kind, "closed")));
    }
    assert!(!tunnel_end_error(&io::Error::new(
        io::ErrorKind::InvalidData,
        "bad data",
    )));

    let (mut writer, mut reader) = tcp_pair();
    let mut pending = b"flush me".to_vec();
    assert_eq!(
        flush_pending_to_stream(&mut writer, &mut pending).unwrap(),
        8
    );
    assert!(pending.is_empty());
    let mut received = [0; 8];
    reader.read_exact(&mut received).unwrap();
    assert_eq!(&received, b"flush me");

    let (store, _trace, id) = tunnel_trace();
    let session = finish_trace(&store, id);
    assert_eq!(session.request_bytes, 0);
}
