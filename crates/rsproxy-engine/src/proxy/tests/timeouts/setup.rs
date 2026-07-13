use super::super::*;

#[test]
fn failed_tls_record_keeps_stage_error_and_host() {
    let err = stage_error("tls", "received fatal alert: ProtocolVersion");
    let record = failed_tls_record("upstream_tls", "origin.test", 0, &err);

    assert_eq!(record.phase, "upstream_tls");
    assert_eq!(record.host, "origin.test");
    assert_eq!(
        record.error.as_deref(),
        Some("stage=tls: received fatal alert: ProtocolVersion")
    );
    assert_eq!(record.protocol, None);
    assert_eq!(record.cipher_suite, None);
}

#[test]
fn client_tls_handshake_timeout_is_absolute_and_restores_socket_timeouts() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let _client = TcpStream::connect(addr).unwrap();
    let (mut server, _) = listener.accept().unwrap();
    let original_timeout = Some(Duration::from_secs(2));
    server.set_read_timeout(original_timeout).unwrap();
    server.set_write_timeout(original_timeout).unwrap();

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(rustls::server::ResolvesServerCertUsingSni::new()));
    let mut connection = ServerConnection::new(Arc::new(config)).unwrap();
    let timeout = Duration::from_millis(40);
    let started = Instant::now();
    let error = complete_client_tls_handshake(&mut connection, &mut server, timeout)
        .expect_err("silent client should not complete a TLS handshake");

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        error.to_string(),
        "stage=client_tls_handshake: timeout after 40ms"
    );
    assert!(is_client_tls_handshake_timeout(&error));
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert_eq!(server.read_timeout().unwrap(), original_timeout);
    assert_eq!(server.write_timeout().unwrap(), original_timeout);

    let protocol_error = client_tls_handshake_io_error(
        io::Error::new(io::ErrorKind::InvalidData, "bad client hello"),
        timeout,
    );
    assert_eq!(protocol_error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        protocol_error.to_string(),
        "stage=client_tls: bad client hello"
    );
    assert!(!is_client_tls_handshake_timeout(&protocol_error));
}

#[test]
fn upstream_tls_handshake_timeout_is_staged_and_recorded() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let (_stream, _) = listener.accept().unwrap();
        let _ = release_rx.recv_timeout(Duration::from_secs(1));
    });
    let mut state = test_state();
    state.config.upstream_tls_handshake_timeout = Duration::from_millis(40);
    let tcp = connect_tcp_with_timeouts(
        &addr.to_string(),
        &state,
        &mut NetworkTimings::default(),
        request_deadline(),
    )
    .unwrap();
    let mut tls_records = Vec::new();
    let started = Instant::now();

    let error = match tls_wrap_upstream_stream(
        UpstreamStream::Tcp(tcp),
        TlsWrapInput {
            tls_host: "127.0.0.1",
            client_identity: None,
            tls_policy: None,
            allow_h2: false,
            state: &state,
            deadline: request_deadline(),
        },
        &mut tls_records,
    ) {
        Err(error) => error,
        Ok(_) => panic!("silent peer unexpectedly completed a TLS handshake"),
    };
    let _ = release_tx.send(());

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(error.to_string(), "stage=tls_handshake: timeout after 40ms");
    assert!(is_upstream_tls_handshake_timeout(&error));
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert_eq!(tls_records.len(), 1);
    assert_eq!(tls_records[0].phase, "upstream_tls");
    assert_eq!(tls_records[0].host, "127.0.0.1");
    assert_eq!(
        tls_records[0].error.as_deref(),
        Some("stage=tls_handshake: timeout after 40ms")
    );
}

#[test]
fn upstream_tls_non_timeout_error_keeps_tls_stage_and_kind() {
    let error = tls_handshake_io_error(
        io::Error::new(io::ErrorKind::InvalidData, "bad certificate"),
        Duration::from_millis(40),
    );

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "stage=tls: bad certificate");
    assert!(!is_upstream_tls_handshake_timeout(&error));
}

#[test]
fn tcp_connect_timeout_and_refusal_remain_distinct() {
    let timeout = tcp_connect_timeout_error(Duration::from_millis(40), "127.0.0.1:9");
    assert_eq!(timeout.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        timeout.to_string(),
        "stage=connect: timeout after 40ms connecting to 127.0.0.1:9"
    );
    assert!(is_upstream_tcp_connect_timeout(&timeout));

    // Freshly closed ports do not report refusal consistently across operating systems. The
    // contract is that staging preserves a real refusal and does not classify it as a timeout.
    let refused = staged_io_error(
        "connect",
        io::Error::new(io::ErrorKind::ConnectionRefused, "connection refused"),
    );
    assert_eq!(refused.kind(), io::ErrorKind::ConnectionRefused);
    assert_eq!(refused.to_string(), "stage=connect: connection refused");
    assert!(!is_upstream_tcp_connect_timeout(&refused));
}
