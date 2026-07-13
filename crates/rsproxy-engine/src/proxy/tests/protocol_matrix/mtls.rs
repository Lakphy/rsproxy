use super::*;
use rcgen::{BasicConstraints, DistinguishedName};
use rustls::server::WebPkiClientVerifier;

const HOST: &str = "mtls.matrix.test";

#[test]
fn upstream_mtls_succeeds_with_client_identity_and_fails_without_it_over_real_tls() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let success_rules = format!(
        "{HOST} host({origin_address}) tls(client-cert=<certs/client.pem>, client-key=<certs/client-key.pem>)"
    );
    let success_state = isolated_state("protocol-mtls", &success_rules);
    let client_roots = install_client_identity(&success_state.config.storage);
    let (cert_path, key_path) = ensure_leaf_certificate(
        &success_state.config.storage.join("ca"),
        success_state.config.ca_material.as_ref().unwrap(),
        HOST,
    )
    .unwrap();
    let verifier = WebPkiClientVerifier::builder(Arc::new(client_roots))
        .build()
        .unwrap();
    let mut server_config = ServerConfig::builder()
        .with_client_cert_verifier(verifier)
        .with_single_cert(
            load_certs(&cert_path).unwrap(),
            load_private_key(&key_path).unwrap(),
        )
        .unwrap();
    server_config.alpn_protocols = vec![HTTP1_ALPN.to_vec()];
    let origin_server = spawn_mtls_origin(origin, Arc::new(server_config));

    let mut missing_state = success_state.clone();
    missing_state.rules = RuleStore::from_compiled(
        &missing_state.config.storage,
        rsproxy_rules::RuleSet::parse("default", &format!("{HOST} host({origin_address})"))
            .unwrap(),
    );
    missing_state.trace = rsproxy_trace::TraceStore::new(8);

    let (success_proxy, success_worker) = spawn_proxy(success_state.clone(), 1);
    let (status, body) = https_request(success_proxy, &success_state);
    assert_eq!(status, 200);
    assert_eq!(body, b"mtls-ok");
    success_worker.join().unwrap();

    let (missing_proxy, missing_worker) = spawn_proxy(missing_state.clone(), 1);
    let (status, body) = https_request(missing_proxy, &missing_state);
    assert_eq!(status, 502);
    let missing_body = String::from_utf8_lossy(&body);
    assert!(
        is_missing_client_cert_error(&missing_body),
        "missing-client-cert response: {missing_body}"
    );
    missing_worker.join().unwrap();
    origin_server.join().unwrap();

    let success = success_state.trace.list(2).pop().unwrap();
    assert_eq!(success.status, Some(200));
    assert!(success.flags.contains(&"upstream-mtls".to_string()));
    assert!(success.error.is_none());
    let missing = missing_state.trace.list(2).pop().unwrap();
    assert_eq!(missing.status, Some(502));
    assert!(!missing.flags.contains(&"upstream-mtls".to_string()));
    assert!(
        missing
            .error
            .as_deref()
            .is_some_and(is_missing_client_cert_error)
    );
    let _ = fs::remove_dir_all(&success_state.config.storage);
}

fn is_missing_client_cert_error(error: &str) -> bool {
    error.contains("stage=request_write")
        || error.contains("stage=response_head")
        || error.contains("stage=tls")
        // macOS may surface the peer's fatal TLS alert as EINVAL when many
        // socket tests run concurrently; the 502 and origin-side rejection
        // above still prove that the anonymous handshake was refused.
        || error.contains("Invalid argument (os error 22)")
}

fn install_client_identity(storage: &Path) -> RootCertStore {
    let cert_dir = storage.join("certs");
    fs::create_dir_all(&cert_dir).unwrap();
    let mut ca_params = CertificateParams::default();
    let mut ca_name = DistinguishedName::new();
    ca_name.push(DnType::CommonName, "rsproxy protocol matrix client CA");
    ca_params.distinguished_name = ca_name;
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca_params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    ca_params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    let ca_key = KeyPair::generate().unwrap();
    let ca_cert = ca_params.self_signed(&ca_key).unwrap();
    let issuer = Issuer::from_params(&ca_params, &ca_key);

    let mut client_params = CertificateParams::default();
    client_params
        .distinguished_name
        .push(DnType::CommonName, "rsproxy protocol matrix client");
    client_params
        .key_usages
        .push(KeyUsagePurpose::DigitalSignature);
    client_params
        .extended_key_usages
        .push(ExtendedKeyUsagePurpose::ClientAuth);
    client_params.use_authority_key_identifier_extension = true;
    let client_key = KeyPair::generate().unwrap();
    let client_cert = client_params.signed_by(&client_key, &issuer).unwrap();
    fs::write(cert_dir.join("client.pem"), client_cert.pem()).unwrap();
    fs::write(cert_dir.join("client-key.pem"), client_key.serialize_pem()).unwrap();

    let mut roots = RootCertStore::empty();
    roots.add(ca_cert.der().clone()).unwrap();
    roots
}

fn spawn_mtls_origin(listener: TcpListener, config: Arc<ServerConfig>) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        for expects_client_cert in [true, false] {
            let (stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut tls = StreamOwned::new(ServerConnection::new(config.clone()).unwrap(), stream);
            let handshake = complete_server_handshake(&mut tls);
            if !expects_client_cert {
                assert!(
                    handshake.is_err(),
                    "origin accepted an anonymous TLS client"
                );
                continue;
            }
            handshake.unwrap();
            assert_eq!(tls.conn.peer_certificates().unwrap().len(), 1);
            let request = http::read_request_head(&mut tls, 64 * 1024, 128)
                .unwrap()
                .unwrap();
            assert_eq!(request.request.target, "/secure");
            tls.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\nConnection: close\r\n\r\nmtls-ok",
            )
            .unwrap();
            tls.flush().unwrap();
        }
    })
}

fn complete_server_handshake(tls: &mut StreamOwned<ServerConnection, TcpStream>) -> io::Result<()> {
    while tls.conn.is_handshaking() {
        tls.conn.complete_io(&mut tls.sock)?;
    }
    Ok(())
}

fn https_request(proxy: std::net::SocketAddr, state: &SharedState) -> (u16, Vec<u8>) {
    let mut client = connect_client(proxy);
    connect_request(&mut client, &format!("{HOST}:443"));
    let mut client = h1_tls_client(client, state, HOST);
    while client.conn.is_handshaking() {
        client.conn.complete_io(&mut client.sock).unwrap();
    }
    client
        .write_all(
            format!("GET /secure HTTP/1.1\r\nHost: {HOST}\r\nConnection: close\r\n\r\n").as_bytes(),
        )
        .unwrap();
    client.flush().unwrap();
    let response = http::read_response_head(&mut client, 64 * 1024, 128).unwrap();
    let body = read_response_body(&mut client, &response.headers)
        .unwrap()
        .body;
    (response.status, body)
}
