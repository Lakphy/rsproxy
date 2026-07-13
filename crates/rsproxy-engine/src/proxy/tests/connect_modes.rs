use super::support::*;
use super::*;
use std::io::{Read, Write};

#[test]
fn only_injected_material_initializes_the_engine_ca() {
    let mut state = test_state();
    state.config.storage = std::env::temp_dir().join(format!(
        "rsproxy-ca-boundary-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let ca_directory = state.config.storage.join("ca");
    fs::create_dir_all(&ca_directory).unwrap();
    let material = test_ca_material();
    fs::write(
        ca_directory.join("rsproxy-root-ca.pem"),
        material.certificate_pem(),
    )
    .unwrap();
    fs::write(
        ca_directory.join("rsproxy-root-ca-key.pem"),
        material.private_key_pem(),
    )
    .unwrap();

    assert!(!ca_initialized(&state));
    state.config.ca_material = Some(material);
    fs::remove_file(ca_directory.join("rsproxy-root-ca.pem")).unwrap();
    fs::remove_file(ca_directory.join("rsproxy-root-ca-key.pem")).unwrap();
    assert!(ca_initialized(&state));

    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn no_mitm_passthrough_wins_even_when_a_ca_is_initialized() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_server = thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        let mut payload = [0u8; 6];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(&payload, b"opaque");
        stream.write_all(b"echo").unwrap();
    });
    let mut state = isolated_state("no-mitm", "");
    state.config.no_mitm = true;
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);

    let mut client = connect_client(proxy_address);
    connect_request(&mut client, &origin_address.to_string());
    client.write_all(b"opaque").unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert_eq!(response, "echo");

    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].flags.contains(&"no-mitm".to_string()));
    assert!(!sessions[0].flags.contains(&"connect-probe-tls".to_string()));
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn passthrough_tunnel_is_pending_until_both_copy_directions_finish() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let (request_seen_sender, request_seen) = std::sync::mpsc::channel();
    let (release_sender, release) = std::sync::mpsc::channel();
    let origin_server = thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        let mut payload = [0u8; 6];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(&payload, b"opaque");
        request_seen_sender.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(2)).unwrap();
        stream.write_all(b"reply").unwrap();
    });
    let mut state = isolated_state("pending-tunnel", "");
    state.config.no_mitm = true;
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);

    let mut client = connect_client(proxy_address);
    connect_request(&mut client, &origin_address.to_string());
    let pending = wait_for_trace_stats(&state.trace, |stats| stats.pending_sessions == 1);
    assert_eq!(pending.sessions, 0);
    assert!(state.trace.list(1).is_empty());

    client.write_all(b"opaque").unwrap();
    client.flush().unwrap();
    request_seen.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(state.trace.stats().pending_sessions, 1);
    release_sender.send(()).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert_eq!(response, "reply");

    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    let session = state.trace.list(1).pop().unwrap();
    assert_eq!(session.request_bytes, 6);
    assert_eq!(session.response_bytes, 5);
    assert!(session.req_body_head.is_empty());
    assert!(session.res_body_head.is_empty());
    assert_eq!(state.trace.stats().pending_sessions, 0);
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn passthrough_connect_failure_finishes_one_event_session_without_orphans() {
    let unavailable = TcpListener::bind("127.0.0.1:0").unwrap();
    let target = unavailable.local_addr().unwrap();
    drop(unavailable);
    let mut state = isolated_state("failed-tunnel", "");
    state.config.no_mitm = true;
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy_address);

    client
        .write_all(format!("CONNECT {target} HTTP/1.1\r\nHost: {target}\r\n\r\n").as_bytes())
        .unwrap();
    client.flush().unwrap();
    let head = http::read_response_head(&mut client, 4096, 32).unwrap();
    assert_eq!(head.status, 502);
    let _ = read_response_body(&mut client, &head.headers).unwrap();
    drop(client);
    proxy_server.join().unwrap();

    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].kind, SessionKind::Tunnel);
    assert_eq!(sessions[0].status, Some(502));
    let stats = state.trace.stats();
    assert_eq!(stats.pending_sessions, 0);
    assert_eq!(stats.orphan_events, 0);
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn hidden_passthrough_tunnel_never_starts_an_event_session() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_server = thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        let mut payload = [0u8; 1];
        stream.read_exact(&mut payload).unwrap();
        stream.write_all(&payload).unwrap();
    });
    let mut state = isolated_state("hidden-tunnel", "127.0.0.1 hide");
    state.config.no_mitm = true;
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);

    let mut client = connect_client(proxy_address);
    connect_request(&mut client, &origin_address.to_string());
    client.write_all(b"x").unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = [0u8; 1];
    client.read_exact(&mut response).unwrap();
    assert_eq!(response, [b'x']);
    drop(client);

    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    assert!(state.trace.list(10).is_empty());
    let stats = state.trace.stats();
    assert_eq!(stats.sessions, 0);
    assert_eq!(stats.pending_sessions, 0);
    assert_eq!(stats.orphan_events, 0);
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn auto_mode_passthroughs_unknown_protocol_after_non_consuming_probe() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_server = thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        let mut payload = [0u8; 4];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(payload, [0x01, 0x02, 0x03, 0x04]);
        stream.write_all(b"binary-ok").unwrap();
    });
    let state = isolated_state("unknown", "");
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);

    let mut client = connect_client(proxy_address);
    connect_request(&mut client, &origin_address.to_string());
    client.write_all(&[0x01, 0x02, 0x03, 0x04]).unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    assert_eq!(response, "binary-ok");

    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 1);
    assert!(
        sessions[0]
            .flags
            .contains(&"connect-probe-unknown".to_string())
    );
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn plaintext_http_inside_connect_reuses_the_http_rule_pipeline() {
    let state = isolated_state("plain-http", "plain.test status(209)");
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy_address);

    connect_request(&mut client, "plain.test:80");
    client
        .write_all(b"GET /inside HTTP/1.1\r\nHost: plain.test\r\nConnection: close\r\n\r\n")
        .unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    client.read_to_string(&mut response).unwrap();
    proxy_server.join().unwrap();

    assert!(response.starts_with("HTTP/1.1 209 OK\r\n"));
    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].url, "http://plain.test:80/inside");
    assert!(
        sessions[0]
            .flags
            .contains(&"connect-probe-http".to_string())
    );
    assert!(sessions[0].flags.contains(&"connect-http".to_string()));
    assert!(!sessions[0].flags.contains(&"mitm".to_string()));
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn tls_clienthello_still_enters_the_mitm_http_pipeline() {
    let state = isolated_state("tls-mitm", "mitm.test status(210)");
    assert!(!state.config.storage.join("ca/rsproxy-root-ca.pem").exists());
    assert!(
        !state
            .config
            .storage
            .join("ca/rsproxy-root-ca-key.pem")
            .exists()
    );
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy_address);
    connect_request(&mut client, "mitm.test:443");

    let mut tls = h1_tls_client(client, &state, "mitm.test");
    tls.write_all(b"GET /secure HTTP/1.1\r\nHost: mitm.test\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    if let Err(error) = tls.read_to_string(&mut response) {
        assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    }
    proxy_server.join().unwrap();

    assert!(response.starts_with("HTTP/1.1 210 OK\r\n"));
    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].flags.contains(&"mitm".to_string()));
    assert!(sessions[0].flags.contains(&"connect-probe-tls".to_string()));
    assert_eq!(sessions[0].tls[0].phase, "client_mitm_tls");
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn failed_mitm_handshake_is_remembered_and_the_retry_passthroughs() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_server = thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        let mut payload = [0u8; 12];
        stream.read_exact(&mut payload).unwrap();
        assert_eq!(&payload, b"pinned-retry");
        stream.write_all(b"recovered").unwrap();
    });
    let state = isolated_state("fallback", "");
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 2);

    let mut first = connect_client(proxy_address);
    connect_request(&mut first, &origin_address.to_string());
    first
        .write_all(&[0x16, 0x03, 0x01, 0x00, 0x01, 0xff])
        .unwrap();
    first.shutdown(Shutdown::Write).unwrap();
    let mut ignored = Vec::new();
    first.read_to_end(&mut ignored).unwrap();

    let mut retry = connect_client(proxy_address);
    connect_request(&mut retry, &origin_address.to_string());
    retry.write_all(b"pinned-retry").unwrap();
    retry.shutdown(Shutdown::Write).unwrap();
    let mut response = String::new();
    retry.read_to_string(&mut response).unwrap();
    assert_eq!(response, "recovered");

    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 2);
    assert!(sessions.iter().any(|session| {
        session
            .flags
            .contains(&"mitm-fallback-remembered".to_string())
    }));
    assert!(sessions.iter().any(|session| {
        session
            .flags
            .contains(&"mitm-fallback-cache-hit".to_string())
    }));
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn strict_mitm_records_failure_without_enabling_fallback() {
    let mut state = isolated_state("strict", "");
    state.config.strict_mitm = true;
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);

    let mut client = connect_client(proxy_address);
    connect_request(&mut client, "strict.test:443");
    client
        .write_all(&[0x16, 0x03, 0x01, 0x00, 0x01, 0xff])
        .unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    let mut ignored = Vec::new();
    client.read_to_end(&mut ignored).unwrap();
    proxy_server.join().unwrap();

    assert!(!state.mitm_failures.lock().unwrap().is_active("strict.test"));
    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 1);
    assert!(sessions[0].flags.contains(&"strict-mitm".to_string()));
    assert!(
        sessions[0]
            .flags
            .contains(&"mitm-fallback-disabled".to_string())
    );
    let _ = fs::remove_dir_all(&state.config.storage);
}

#[test]
fn mitm_handshake_timeout_finishes_the_tunnel_event_session() {
    let mut state = isolated_state("mitm-timeout", "");
    state.config.client_tls_handshake_timeout = Duration::from_millis(40);
    let (proxy_address, proxy_server) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy_address);
    connect_request(&mut client, "timeout.test:443");

    let started = Instant::now();
    client.write_all(&[0x16, 0x03, 0x01, 0x00, 0x10]).unwrap();
    client.flush().unwrap();
    let mut ignored = Vec::new();
    client.read_to_end(&mut ignored).unwrap();
    assert!(started.elapsed() >= Duration::from_millis(25));
    drop(client);
    proxy_server.join().unwrap();

    let sessions = state.trace.list(10);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].kind, SessionKind::Tunnel);
    assert_eq!(sessions[0].status, Some(408));
    assert!(
        sessions[0]
            .flags
            .contains(&"client-tls-handshake-timeout".to_string())
    );
    let stats = state.trace.stats();
    assert_eq!(stats.pending_sessions, 0);
    assert_eq!(stats.orphan_events, 0);
    let _ = fs::remove_dir_all(&state.config.storage);
}
