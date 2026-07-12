use super::super::*;
use std::io::Cursor;

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

#[test]
fn unix_api_endpoint_parsing_is_explicit() {
    assert_eq!(
        unix_api_path("unix:/tmp/rsproxy.sock"),
        Some("/tmp/rsproxy.sock")
    );
    assert_eq!(
        unix_api_path("unix:///tmp/rsproxy.sock"),
        Some("/tmp/rsproxy.sock")
    );
    assert_eq!(unix_api_path("127.0.0.1:8900"), None);
    assert_eq!(api_display("127.0.0.1:8900"), "http://127.0.0.1:8900");
    assert_eq!(
        api_display("unix:/tmp/rsproxy.sock"),
        "unix:/tmp/rsproxy.sock"
    );
    assert_eq!(
        crate::app::windows_pipe_path("pipe:rsproxy"),
        Some("rsproxy")
    );
    assert_eq!(
        crate::app::windows_pipe_path(r"npipe:\\.\pipe\rsproxy"),
        Some(r"\\.\pipe\rsproxy")
    );
    assert_eq!(crate::app::windows_pipe_path("127.0.0.1:8900"), None);
    assert_eq!(api_display("pipe:rsproxy"), "pipe:rsproxy");
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
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\n\r\n\n{\"id\":1}\n{\"id\":2}\n{\"id\":3}\n";
    let mut stream = ScriptedApiStream {
        response: Cursor::new(response.to_vec()),
        request: Vec::new(),
    };
    let mut lines = Vec::new();
    let mut consume = |line: &str| {
        lines.push(line.to_string());
        Ok(lines.len() < 2)
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
fn tcp_api_token_is_generated_secured_reused_and_overridden() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-api-token-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    let _ = fs::remove_dir_all(&storage);
    let mut generated = AppConfig {
        storage: storage.clone(),
        api: "127.0.0.1:18999".to_string(),
        ..AppConfig::default()
    };
    prepare_server_api_auth(&mut generated).unwrap();
    let first = generated.api_token.clone().unwrap();
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

    let mut reused = AppConfig {
        storage: storage.clone(),
        api: "127.0.0.1:18999".to_string(),
        ..AppConfig::default()
    };
    prepare_server_api_auth(&mut reused).unwrap();
    assert_eq!(reused.api_token.as_deref(), Some(first.as_str()));

    let replacement = "fedcba9876543210fedcba9876543210";
    reused.api_token = Some(replacement.to_string());
    prepare_server_api_auth(&mut reused).unwrap();
    assert_eq!(reused.api_token.as_deref(), Some(replacement));
    assert_eq!(fs::read_to_string(&path).unwrap(), replacement);

    let mut unix = AppConfig {
        storage: storage.clone(),
        api: "unix:/tmp/rsproxy-test.sock".to_string(),
        api_token: Some(replacement.to_string()),
        ..AppConfig::default()
    };
    prepare_server_api_auth(&mut unix).unwrap();
    assert_eq!(unix.api_token, None);
    let _ = fs::remove_dir_all(storage);
}
