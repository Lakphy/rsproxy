use rsproxy_net::read_response_head_buffered;
use std::io::{self, BufReader, Cursor, Read};

#[test]
fn buffered_response_head_preserves_a_buffered_body() {
    let raw = b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\nbody";
    let mut reader = BufReader::with_capacity(raw.len(), Cursor::new(raw));

    let head = read_response_head_buffered(&mut reader, 4096, 8).unwrap();
    let mut body = Vec::new();
    reader.read_to_end(&mut body).unwrap();

    assert_eq!(head.status, 200);
    assert_eq!(body, b"body");
}

#[test]
fn buffered_response_head_finds_a_split_terminator() {
    let raw = b"HTTP/1.1 204 No Content\r\nX-Test: yes\r\n\r\nnext";
    let mut reader = BufReader::with_capacity(2, Cursor::new(raw));

    let head = read_response_head_buffered(&mut reader, 4096, 8).unwrap();
    let mut remaining = Vec::new();
    reader.read_to_end(&mut remaining).unwrap();

    assert_eq!(head.status, 204);
    assert_eq!(
        head.headers,
        vec![("X-Test".to_string(), "yes".to_string())]
    );
    assert_eq!(remaining, b"next");
}

#[test]
fn buffered_response_head_enforces_eof_and_size_limits() {
    let mut incomplete = BufReader::new(Cursor::new(b"HTTP/1.1 200 OK\r\n"));
    let error = read_response_head_buffered(&mut incomplete, 4096, 8).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);

    let mut oversized = BufReader::new(Cursor::new(b"HTTP/1.1 200 OK\r\n\r\n"));
    let error = read_response_head_buffered(&mut oversized, 8, 8).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(error.to_string(), "header size limit exceeded");
}
