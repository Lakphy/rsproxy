use super::*;
use crate::{CaMaterial, ProxyConfig, SharedState, issue_leaf_certificate};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use rsproxy_trace::{Session, SessionKind};
use rustls::{ServerConfig, ServerConnection};
use std::fs;
use std::net::TcpListener;

#[test]
fn https_replay_uses_tls_and_returns_the_origin_response() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-replay-https-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let ca = test_ca();
    let leaf =
        issue_leaf_certificate(ca.certificate_pem(), ca.private_key_pem(), "127.0.0.1").unwrap();
    fs::create_dir_all(&storage).unwrap();
    let cert_path = storage.join("origin.pem");
    let key_path = storage.join("origin-key.pem");
    fs::write(&cert_path, leaf.certificate_pem).unwrap();
    fs::write(&key_path, leaf.private_key_pem).unwrap();
    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(
            crate::proxy::tls::load_certs(&cert_path).unwrap(),
            crate::proxy::tls::load_private_key(&key_path).unwrap(),
        )
        .unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (tcp, _) = listener.accept().unwrap();
        let connection = ServerConnection::new(Arc::new(server_config)).unwrap();
        let mut tls = StreamOwned::new(connection, tcp);
        let mut request = Vec::new();
        let mut byte = [0u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            tls.read_exact(&mut byte).unwrap();
            request.push(byte[0]);
        }
        assert!(String::from_utf8_lossy(&request).starts_with("GET /secure HTTP/1.1\r\n"));
        tls.write_all(
            b"HTTP/1.1 202 Accepted\r\nContent-Length: 6\r\nConnection: close\r\n\r\nsecure",
        )
        .unwrap();
        tls.flush().unwrap();
    });

    let mut config = ProxyConfig::new(&storage);
    config.ca_material = Some(ca);
    config.trace_disk_budget = 0;
    config.request_total_timeout = Duration::from_secs(5);
    let state = SharedState::new(config).unwrap();
    let mut session = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        format!("https://{origin}/secure"),
        "test".to_string(),
    );
    session
        .req_headers
        .push(("Host".to_string(), origin.to_string()));
    let replay = state.handle().replay(&session).unwrap();
    server.join().unwrap();
    assert_eq!(replay.status, 202);
    assert_eq!(replay.response_bytes, 6);
    assert_eq!(replay.body_head, b"secure");
    let _ = fs::remove_dir_all(storage);
}

fn test_ca() -> CaMaterial {
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "rsproxy replay test CA");
    params.distinguished_name = name;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    CaMaterial::from_pem(cert.pem(), key.serialize_pem())
}
