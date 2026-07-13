use super::*;
use bytes::Bytes;
use http_body::Body;
use hyper::body::Frame;
use rcgen::{BasicConstraints, DistinguishedName, KeyPair};
use std::convert::Infallible;
use std::io::{self, Read, Write};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};

pub(super) fn isolated_state(name: &str, rules: &str) -> SharedState {
    let mut state = test_state();
    state.config.storage = std::env::temp_dir().join(format!(
        "rsproxy-connect-{name}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        RuleSet::parse("default", rules).unwrap(),
    );
    install_test_ca(&mut state);
    state
}

fn install_test_ca(state: &mut SharedState) {
    state.config.ca_material = Some(test_ca_material());
}

pub(super) fn test_ca_material() -> crate::CaMaterial {
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "rsproxy connect test CA");
    params.distinguished_name = name;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    let key = KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    crate::CaMaterial::from_pem(cert.pem(), key.serialize_pem())
}

pub(super) fn spawn_proxy(
    state: SharedState,
    connections: usize,
) -> (SocketAddr, thread::JoinHandle<()>) {
    spawn_proxy_with_h2_disconnect_policy(state, connections, false)
}

// Stage 2 is intentionally opt-in. CPU-stress iteration 2 still surfaced
// NotConnected in the protocol-matrix HTTP/2 large-header test after the
// client-side graceful await was added.
pub(super) fn spawn_proxy_allowing_h2_disconnect(
    state: SharedState,
    connections: usize,
) -> (SocketAddr, thread::JoinHandle<()>) {
    spawn_proxy_with_h2_disconnect_policy(state, connections, true)
}

fn spawn_proxy_with_h2_disconnect_policy(
    state: SharedState,
    connections: usize,
    allow_h2_disconnect: bool,
) -> (SocketAddr, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        for _ in 0..connections {
            let (stream, _) = listener.accept().unwrap();
            match handle_client(stream, state.clone()) {
                Ok(()) => {}
                Err(error) if allow_h2_disconnect && is_expected_h2_disconnect(&error) => {}
                Err(error) => panic!("unexpected proxy server error: {error:?}"),
            }
        }
    });
    (address, server)
}

fn is_expected_h2_disconnect(error: &io::Error) -> bool {
    is_expected_h2_disconnect_message(&format!("{error:?}"))
}

fn is_expected_h2_disconnect_message(message: &str) -> bool {
    message.contains("hyper::Error(Io")
        && [
            "NotConnected",
            "ConnectionReset",
            "BrokenPipe",
            "UnexpectedEof",
        ]
        .iter()
        .any(|kind| message.contains(kind))
}

#[test]
fn h2_disconnect_allowlist_is_limited_to_expected_hyper_io_errors() {
    for kind in [
        "NotConnected",
        "ConnectionReset",
        "BrokenPipe",
        "UnexpectedEof",
    ] {
        assert!(is_expected_h2_disconnect_message(&format!(
            "hyper::Error(Io, Kind({kind}))"
        )));
    }
    for message in [
        "hyper::Error(Http2, Kind(NotConnected))",
        "Custom { kind: BrokenPipe }",
        "hyper::Error(Io, Kind(ConnectionAborted))",
        "callback failed",
    ] {
        assert!(!is_expected_h2_disconnect_message(message));
    }
}

pub(super) fn connect_request(client: &mut TcpStream, target: &str) {
    client
        .write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes())
        .unwrap();
    let head = read_head(client);
    assert!(head.starts_with("HTTP/1.1 200 Connection Established\r\n"));
}

fn read_head(stream: &mut TcpStream) -> String {
    let mut bytes = Vec::new();
    let mut byte = [0u8; 1];
    while !bytes.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).unwrap();
        bytes.push(byte[0]);
        assert!(bytes.len() < 16 * 1024);
    }
    String::from_utf8(bytes).unwrap()
}

pub(super) fn connect_client(address: SocketAddr) -> TcpStream {
    let client = TcpStream::connect(address).unwrap();
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client
        .set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    client
}

pub(super) fn h2_tls_client(
    stream: TcpStream,
    state: &SharedState,
    server_name: &str,
) -> StreamOwned<ClientConnection, TcpStream> {
    tls_client(stream, state, server_name, H2_ALPN)
}

pub(super) fn h1_tls_client(
    stream: TcpStream,
    state: &SharedState,
    server_name: &str,
) -> StreamOwned<ClientConnection, TcpStream> {
    tls_client(stream, state, server_name, HTTP1_ALPN)
}

fn tls_client(
    stream: TcpStream,
    state: &SharedState,
    server_name: &str,
    alpn: &[u8],
) -> StreamOwned<ClientConnection, TcpStream> {
    let material = state.config.ca_material.as_ref().unwrap();
    let mut roots = RootCertStore::empty();
    for certificate in load_certs_from_pem(material.certificate_pem()).unwrap() {
        roots.add(certificate).unwrap();
    }
    let mut config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    config.alpn_protocols = vec![alpn.to_vec()];
    let connection = ClientConnection::new(
        Arc::new(config),
        ServerName::try_from(server_name.to_string()).unwrap(),
    )
    .unwrap();
    StreamOwned::new(connection, stream)
}

pub(super) struct ChannelRequestBody {
    receiver: tokio::sync::mpsc::Receiver<Result<Frame<Bytes>, Infallible>>,
}

impl Body for ChannelRequestBody {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        Pin::new(&mut self.receiver).poll_recv(context)
    }

    fn is_end_stream(&self) -> bool {
        self.receiver.is_closed() && self.receiver.is_empty()
    }
}

pub(super) fn channel_request_body(
    capacity: usize,
) -> (
    tokio::sync::mpsc::Sender<Result<Frame<Bytes>, Infallible>>,
    ChannelRequestBody,
) {
    let (sender, receiver) = tokio::sync::mpsc::channel(capacity);
    (sender, ChannelRequestBody { receiver })
}

pub(super) fn wait_for_trace_stats(
    store: &rsproxy_trace::TraceStore,
    predicate: impl Fn(&rsproxy_trace::TraceStats) -> bool,
) -> rsproxy_trace::TraceStats {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let stats = store.stats();
        if predicate(&stats) {
            return stats;
        }
        assert!(
            Instant::now() < deadline,
            "trace stats condition was not met: {stats:?}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}
