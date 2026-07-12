use super::support::{request, response_body, test_state};
use crate::control::routes::dispatch;
use crate::control::{self, router};
use rsproxy_trace::{
    FrameDataEncoding, FrameDirection, FrameRecord, Session, SessionKind, TlsRecord,
};
use std::fs;
use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

fn status(response: &[u8]) -> u16 {
    std::str::from_utf8(response)
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}

fn populated_session(url: String) -> Session {
    let mut session = Session::new(
        SessionKind::Http,
        "POST".to_string(),
        url,
        "127.0.0.1:12345".to_string(),
    );
    session.status = Some(202);
    session.upstream = Some("origin.test:80".to_string());
    session.request_bytes = 4;
    session.response_bytes = 4;
    session.req_headers = vec![
        ("Proxy-Connection".to_string(), "keep-alive".to_string()),
        ("Connection".to_string(), "keep-alive".to_string()),
        ("Content-Length".to_string(), "99".to_string()),
        ("X-Test".to_string(), "yes".to_string()),
    ];
    session.req_body_head = b"ping".to_vec();
    session.res_headers = vec![("Content-Type".to_string(), "text/plain".to_string())];
    session.res_body_head = b"pong".to_vec();
    session.flags = vec!["fixture".to_string()];
    session.frames = vec![FrameRecord::new(
        FrameDirection::ServerToClient,
        1,
        "text",
        true,
        b"hello",
        5,
        FrameDataEncoding::Utf8,
    )];
    session.tls = vec![TlsRecord {
        phase: "origin".to_string(),
        host: "origin.test".to_string(),
        handshake_ms: 1,
        peer_certificates: 1,
        protocol: Some("TLSv1.3".to_string()),
        cipher_suite: Some("TLS_AES_128_GCM_SHA256".to_string()),
        alpn: Some("http/1.1".to_string()),
        error: None,
    }];
    session.finish();
    session
}

#[test]
fn value_ca_trace_and_fallback_routes_cover_success_and_error_contracts() {
    let state = test_state();
    let mut response = Vec::new();
    dispatch(&mut response, &request("GET", "/api/values", &[]), &state).unwrap();
    assert_eq!(response_body(&response), "[]");

    for (method, path, body) in [
        ("PUT", "/api/values/alpha", b"one".as_slice()),
        ("POST", "/api/values/beta%2Ekey", b"two".as_slice()),
    ] {
        let mut response = Vec::new();
        dispatch(&mut response, &request(method, path, body), &state).unwrap();
        assert_eq!(status(&response), 200);
    }
    fs::create_dir_all(state.config.storage.join("values/nested")).unwrap();

    let mut list = Vec::new();
    dispatch(&mut list, &request("GET", "/api/values", &[]), &state).unwrap();
    assert_eq!(response_body(&list), "[\"alpha\",\"beta.key\"]");
    let mut list_text = Vec::new();
    dispatch(
        &mut list_text,
        &request("GET", "/api/values.txt", &[]),
        &state,
    )
    .unwrap();
    assert_eq!(response_body(&list_text), "alpha\nbeta.key");

    for (path, expected_status, expected_body) in [
        ("/api/values/beta%2Ekey", 200, "two"),
        ("/api/values/missing", 404, "{\"error\":\"not found\"}"),
        ("/api/values/bad%2Fkey", 400, "{\"error\":\"invalid key\"}"),
    ] {
        let mut response = Vec::new();
        dispatch(&mut response, &request("GET", path, &[]), &state).unwrap();
        assert_eq!(status(&response), expected_status);
        assert_eq!(response_body(&response), expected_body);
    }
    let mut deleted = Vec::new();
    dispatch(
        &mut deleted,
        &request("DELETE", "/api/values/alpha", &[]),
        &state,
    )
    .unwrap();
    assert!(!state.config.storage.join("values/alpha").exists());

    let mut ca = Vec::new();
    dispatch(&mut ca, &request("GET", "/api/ca/root.pem", &[]), &state).unwrap();
    assert_eq!(status(&ca), 404);
    fs::create_dir_all(state.config.storage.join("ca")).unwrap();
    fs::write(
        state.config.storage.join("ca/rsproxy-root-ca.pem"),
        b"fixture-ca",
    )
    .unwrap();
    let mut ca = Vec::new();
    dispatch(&mut ca, &request("GET", "/rsproxy.crt", &[]), &state).unwrap();
    assert_eq!(response_body(&ca), "fixture-ca");

    state
        .trace
        .record(populated_session("http://example.test/".to_string()));
    let mut stats = Vec::new();
    dispatch(&mut stats, &request("GET", "/api/trace/stats", &[]), &state).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(response_body(&stats)).unwrap()["sessions"],
        1
    );
    let mut cleared = Vec::new();
    dispatch(
        &mut cleared,
        &request("POST", "/api/trace/clear", &[]),
        &state,
    )
    .unwrap();
    assert_eq!(state.trace.stats().sessions, 0);

    let mut missing = Vec::new();
    dispatch(&mut missing, &request("PATCH", "/unknown", &[]), &state).unwrap();
    assert_eq!(status(&missing), 404);
    fs::remove_dir_all(&state.config.storage).unwrap();
}

#[test]
fn session_routes_list_follow_detail_export_and_missing_spill() {
    let state = test_state();
    state
        .trace
        .record(populated_session("http://example.test/session".to_string()));
    let id = state.trace.list(1)[0].id;

    for path in [
        "/api/sessions?limit=bad",
        "/api/sessions.txt?limit=1",
        "/api/sessions.ndjson?after=0&limit=1",
        "/api/sessions/export.json",
        "/api/sessions/export.har",
        &format!("/api/sessions/{id}"),
    ] {
        let mut response = Vec::new();
        dispatch(&mut response, &request("GET", path, &[]), &state).unwrap();
        assert_eq!(status(&response), 200, "{path}");
        assert!(response_body(&response).contains("example.test"), "{path}");
    }
    for path in [
        "/api/sessions/not-a-number",
        "/api/sessions/999999",
        "/api/sessions/spill.ndjson",
    ] {
        let mut response = Vec::new();
        dispatch(&mut response, &request("GET", path, &[]), &state).unwrap();
        assert_eq!(status(&response), 404, "{path}");
    }
    fs::remove_dir_all(&state.config.storage).unwrap_or(());
}

#[test]
fn replay_route_sends_sanitized_request_to_real_origin_and_maps_errors() {
    let state = test_state();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut head = String::new();
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            head.push_str(&line);
            if line == "\r\n" {
                break;
            }
        }
        assert!(head.starts_with("POST /replay?x=1 HTTP/1.1\r\n"));
        assert!(head.contains(&format!("Host: {origin}\r\n")));
        assert!(head.contains("Connection: close\r\n"));
        assert!(head.contains("Content-Length: 4\r\n"));
        assert!(!head.to_ascii_lowercase().contains("proxy-connection"));
        let mut body = [0u8; 4];
        reader.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"ping");
        let mut writer = stream;
        writer
            .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 4\r\n\r\npong")
            .unwrap();
    });
    state
        .trace
        .record(populated_session(format!("http://{origin}/replay?x=1")));
    let id = state.trace.list(1)[0].id;
    let mut replay = Vec::new();
    dispatch(
        &mut replay,
        &request("POST", &format!("/api/replay/{id}"), &[]),
        &state,
    )
    .unwrap();
    server.join().unwrap();
    assert_eq!(status(&replay), 200);
    let body: serde_json::Value = serde_json::from_str(response_body(&replay)).unwrap();
    assert_eq!(body["status"], 201);
    assert_eq!(body["response_bytes"], 4);
    assert_eq!(body["body_head"], "pong");

    state
        .trace
        .record(populated_session("https://example.test/".to_string()));
    let https_id = state.trace.list(1)[0].id;
    let mut unsupported = Vec::new();
    dispatch(
        &mut unsupported,
        &request("POST", &format!("/api/replay/{https_id}"), &[]),
        &state,
    )
    .unwrap();
    assert_eq!(status(&unsupported), 502);
    let mut missing = Vec::new();
    dispatch(
        &mut missing,
        &request("POST", "/api/replay/not-found", &[]),
        &state,
    )
    .unwrap();
    assert_eq!(status(&missing), 404);
    fs::remove_dir_all(&state.config.storage).unwrap_or(());
}

struct MemoryStream {
    input: Cursor<Vec<u8>>,
    output: Arc<Mutex<Vec<u8>>>,
}

impl MemoryStream {
    fn new(input: impl Into<Vec<u8>>) -> Self {
        Self {
            input: Cursor::new(input.into()),
            output: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl Read for MemoryStream {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        self.input.read(buffer)
    }
}

impl Write for MemoryStream {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.output.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn router_handles_eof_authentication_and_authorized_dispatch() {
    let state = test_state();
    router::handle(MemoryStream::new(Vec::new()), state.clone()).unwrap();

    let mut protected = state.clone();
    protected.config.api_token = Some("secret".to_string());
    let unauthorized =
        MemoryStream::new(b"GET /api/status HTTP/1.1\r\nHost: local\r\n\r\n".to_vec());
    let output = Arc::clone(&unauthorized.output);
    router::handle(unauthorized, protected.clone()).unwrap();
    assert!(
        String::from_utf8(output.lock().unwrap().clone())
            .unwrap()
            .starts_with("HTTP/1.1 401")
    );

    let authorized = MemoryStream::new(
        b"GET /unknown HTTP/1.1\r\nHost: local\r\nAuthorization: Bearer secret\r\n\r\n".to_vec(),
    );
    let output = Arc::clone(&authorized.output);
    router::handle(authorized, protected).unwrap();
    assert!(
        String::from_utf8(output.lock().unwrap().clone())
            .unwrap()
            .starts_with("HTTP/1.1 404")
    );
    fs::remove_dir_all(&state.config.storage).unwrap_or(());
}

#[test]
fn control_bind_reports_tcp_and_private_unix_endpoints() {
    let tcp = control::bind("127.0.0.1:0").unwrap();
    assert!(tcp.endpoint().unwrap().starts_with("127.0.0.1:"));

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let root = std::path::Path::new("/tmp").join(format!("rspc-{}", std::process::id()));
        let path = root.join("c.sock");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"stale").unwrap();
        let unix = control::bind(&format!("unix:{}", path.display())).unwrap();
        assert_eq!(unix.endpoint().unwrap(), format!("unix:{}", path.display()));
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        drop(unix);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(not(windows))]
    assert!(control::bind("pipe:rsproxy-test").is_err());
}
