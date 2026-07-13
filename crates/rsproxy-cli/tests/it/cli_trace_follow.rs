use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::process::Command;

#[test]
fn trace_follow_consumes_the_live_ndjson_stream_and_stops_at_count() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        assert!(request_line.starts_with("GET /api/sessions/follow?"));
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" {
                break;
            }
        }
        stream
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nConnection: close\r\n\r\n\n{\"id\":1,\"url\":\"http://example.test/one\"}\n{\"id\":2,\"url\":\"http://example.test/two\"}\n",
            )
            .unwrap();
        stream.flush().unwrap();
    });

    let api = address.to_string();
    let output = Command::new(env!("CARGO_BIN_EXE_rsproxy"))
        .args([
            "trace",
            "follow",
            "--count",
            "2",
            "--poll-ms",
            "100",
            "--api",
            &api,
        ])
        .output()
        .expect("rsproxy trace follow should run");
    server.join().unwrap();

    assert!(
        output.status.success(),
        "trace follow failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("http://example.test/one"));
    assert!(stdout.contains("http://example.test/two"));
    assert_eq!(stdout.lines().count(), 2);
}
