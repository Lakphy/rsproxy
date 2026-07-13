use super::*;
use std::fs;
use std::io::{self, Cursor};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

struct ScriptedApiStream {
    response: Cursor<Vec<u8>>,
    request: Vec<u8>,
}

impl Read for ScriptedApiStream {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        self.response.read(buffer)
    }
}

impl Write for ScriptedApiStream {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.request.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn temp_storage(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock must be after the Unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "rsproxy-control-{name}-{}-{nonce}",
        std::process::id()
    ))
}

#[test]
fn api_request_text_includes_bearer_token_only_when_configured() {
    let authenticated = api_request_text(
        "GET",
        "127.0.0.1:8900",
        "/api/status",
        "",
        Some("0123456789abcdef"),
    );
    assert!(authenticated.contains("Authorization: Bearer 0123456789abcdef\r\n"));

    let peer_authenticated =
        api_request_text("GET", "unix:/tmp/rsproxy.sock", "/api/status", "", None);
    assert!(!peer_authenticated.contains("Authorization:"));
}

#[test]
fn streaming_api_reader_skips_heartbeats_and_stops_at_the_requested_count() {
    set_api_token(None);
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\n\r\n\n{\"id\":1}\n{\"id\":2}\n{\"id\":3}\n";
    let mut stream = ScriptedApiStream {
        response: Cursor::new(response.to_vec()),
        request: Vec::new(),
    };
    let mut lines = Vec::new();
    let mut consume = |line: &str| {
        lines.push(line.to_string());
        lines.len() < 2
    };

    api_stream_lines_from(
        &mut stream,
        "127.0.0.1:8900",
        "/api/sessions/follow",
        &mut consume,
    )
    .unwrap();

    assert_eq!(lines, vec!["{\"id\":1}", "{\"id\":2}"]);
    assert!(
        String::from_utf8(stream.request)
            .unwrap()
            .starts_with("GET /api/sessions/follow HTTP/1.1\r\n")
    );
}

#[test]
fn request_errors_preserve_http_status_and_body_display() {
    let response =
        b"HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\n\r\n{\"error\":\"conflict\"}";
    let mut stream = ScriptedApiStream {
        response: Cursor::new(response.to_vec()),
        request: Vec::new(),
    };

    let error = api_request_stream(
        &mut stream,
        "127.0.0.1:8900",
        "POST",
        "/api/rules/default",
        "",
    )
    .unwrap_err();

    match error {
        ControlError::HttpStatus { status, body } => {
            assert_eq!(status, 409);
            assert_eq!(body, "{\"error\":\"conflict\"}");
        }
        other => panic!("expected HTTP status error, got {other:?}"),
    }
}

#[test]
fn streaming_errors_preserve_http_status_and_body() {
    let response =
        b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\n\r\n{\"error\":\"unauthorized\"}";
    let mut stream = ScriptedApiStream {
        response: Cursor::new(response.to_vec()),
        request: Vec::new(),
    };
    let mut on_line =
        |_: &str| -> bool { panic!("error responses must not invoke the stream callback") };

    let error = api_stream_lines_from(
        &mut stream,
        "127.0.0.1:8900",
        "/api/sessions/follow",
        &mut on_line,
    )
    .unwrap_err();

    match error {
        ControlError::HttpStatus { status, body } => {
            assert_eq!(status, 401);
            assert_eq!(body, "{\"error\":\"unauthorized\"}");
        }
        other => panic!("expected HTTP status error, got {other:?}"),
    }
}

#[test]
fn tcp_api_token_is_generated_secured_reused_and_overridden() {
    let storage = temp_storage("auth");
    let _ = fs::remove_dir_all(&storage);
    let api = "127.0.0.1:18999";

    let mut generated = None;
    prepare_server_api_auth(api, &storage, &mut generated).unwrap();
    let first = generated.unwrap();
    assert_eq!(first.len(), 64);
    let path = api_token_path(&storage);
    assert_eq!(fs::read_to_string(&path).unwrap(), first);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    let mut reused = None;
    prepare_server_api_auth(api, &storage, &mut reused).unwrap();
    assert_eq!(reused.as_deref(), Some(first.as_str()));

    let replacement = "fedcba9876543210fedcba9876543210";
    reused = Some(replacement.to_string());
    prepare_server_api_auth(api, &storage, &mut reused).unwrap();
    assert_eq!(reused.as_deref(), Some(replacement));
    assert_eq!(fs::read_to_string(&path).unwrap(), replacement);

    prepare_server_api_auth("unix:/tmp/rsproxy-test.sock", &storage, &mut reused).unwrap();
    assert_eq!(reused, None);
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn client_api_token_resolution_preserves_precedence_and_local_peer_auth() {
    let storage = temp_storage("resolution");
    let mut stored = Some("stored-token-0123456789".to_string());
    prepare_server_api_auth("127.0.0.1:18999", &storage, &mut stored).unwrap();

    let resolved = resolve_client_api_token(
        "127.0.0.1:18999",
        &storage,
        Some("explicit-token-0123456789".to_string()),
        Some("environment-token-0123456789".to_string()),
        Some("configured-token-0123456789".to_string()),
    )
    .unwrap();
    assert_eq!(resolved.as_deref(), Some("explicit-token-0123456789"));

    let from_storage =
        resolve_client_api_token("127.0.0.1:18999", &storage, None, None, None).unwrap();
    assert_eq!(from_storage, stored);

    let local = resolve_client_api_token(
        "unix:/tmp/rsproxy-test.sock",
        &storage,
        Some("explicit-token-0123456789".to_string()),
        None,
        None,
    )
    .unwrap();
    assert_eq!(local, None);
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn stored_token_resolution_distinguishes_missing_and_invalid_files() {
    let storage = temp_storage("stored-token-errors");
    let _ = fs::remove_dir_all(&storage);

    assert_eq!(
        resolve_client_api_token("127.0.0.1:18999", &storage, None, None, None).unwrap(),
        None
    );

    let path = api_token_path(&storage);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "short").unwrap();
    let error = resolve_client_api_token("127.0.0.1:18999", &storage, None, None, None)
        .expect_err("an invalid stored token must remain classified");
    assert!(matches!(error, ControlError::Authentication(_)));

    let _ = fs::remove_dir_all(storage);
}
