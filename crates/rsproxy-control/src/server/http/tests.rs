use super::*;
use std::io::Cursor;

#[test]
fn parses_content_length_and_chunked_control_requests() {
    let mut fixed = Cursor::new(
        b"POST /api/rules/default HTTP/1.1\r\nHost: local\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
    );
    let request = read_request(&mut fixed, 4096, 16, 4096).unwrap().unwrap();
    assert_eq!(request.body, b"abc");

    let mut chunked = Cursor::new(
        b"POST /api/rules/default HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nX-End: yes\r\n\r\n"
            .to_vec(),
    );
    let request = read_request(&mut chunked, 4096, 16, 4096).unwrap().unwrap();
    assert_eq!(request.body, b"abc");
}

#[test]
fn rejects_ambiguous_framing_and_enforces_head_limits() {
    let mut ambiguous = Cursor::new(
        b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
    );
    assert!(read_request(&mut ambiguous, 4096, 16, 4096).is_err());

    let mut oversized = Cursor::new(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n".to_vec());
    assert!(read_request(&mut oversized, 8, 16, 4096).is_err());

    let mut huge_declaration =
        Cursor::new(b"POST / HTTP/1.1\r\nContent-Length: 18446744073709551615\r\n\r\n".to_vec());
    assert!(read_request(&mut huge_declaration, 4096, 16, 1024).is_err());
}

fn request_error(input: impl Into<Vec<u8>>, head: usize, headers: usize, body: usize) -> String {
    let mut input = Cursor::new(input.into());
    read_request(&mut input, head, headers, body)
        .unwrap_err()
        .to_string()
}

#[test]
fn rejects_malformed_request_lines_headers_and_framing() {
    assert!(
        request_error(b"GET / HTTP/1.1 extra\r\n\r\n".to_vec(), 4096, 16, 4096)
            .contains("invalid control request line")
    );
    assert!(
        request_error(
            b"GET / HTTP/1.1\r\nHost: local\r\n\r\n".to_vec(),
            4096,
            0,
            4096
        )
        .contains("header count exceeds limit")
    );
    assert!(
        request_error(
            b"GET / HTTP/1.1\r\nBad Name: value\r\n\r\n".to_vec(),
            4096,
            16,
            4096
        )
        .contains("invalid control request header name")
    );
    assert!(
        request_error(
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 2\r\n\r\n".to_vec(),
            4096,
            16,
            4096
        )
        .contains("conflicting control request Content-Length")
    );
    assert!(
        request_error(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: gzip\r\n\r\n".to_vec(),
            4096,
            16,
            4096
        )
        .contains("unsupported control request Transfer-Encoding")
    );
    assert!(
        request_error(
            b"POST / HTTP/1.1\r\nContent-Length: 3\r\n\r\na".to_vec(),
            4096,
            16,
            4096
        )
        .contains("truncated control request body")
    );
    assert!(
        request_error(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\n\r\n".to_vec(),
            4096,
            16,
            2
        )
        .contains("body exceeds configured limit")
    );
    assert!(
        request_error(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1\r\naXX0\r\n\r\n".to_vec(),
            4096,
            16,
            4096
        )
        .contains("invalid control request chunk terminator")
    );
}

#[test]
fn rejects_truncated_heads_and_chunked_trailer_limits() {
    let mut truncated = Cursor::new(b"GET / HTTP/1.1\r\nHost: local".to_vec());
    let error = read_request(&mut truncated, 4096, 16, 4096).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert!(error.to_string().contains("truncated control request head"));

    struct ErrorReader;
    impl Read for ErrorReader {
        fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }
    }
    let error = read_request(&mut ErrorReader, 4096, 16, 4096).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);

    let long_trailers = format!(
        "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-One: {}\r\nX-Two: {}\r\n\r\n",
        "a".repeat(25),
        "b".repeat(25)
    );
    assert!(
        request_error(long_trailers.into_bytes(), 64, 16, 4096)
            .contains("trailers exceed configured limit")
    );

    assert!(
        request_error(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-One: yes\r\nX-Two: yes\r\n\r\n"
                .to_vec(),
            4096,
            1,
            4096
        )
        .contains("trailer count exceeds limit")
    );

    let long_chunk_line = format!(
        "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n1;{}\r\na\r\n0\r\n\r\n",
        "x".repeat(80)
    );
    assert!(
        request_error(long_chunk_line.into_bytes(), 64, 16, 4096)
            .contains("framing line exceeds limit")
    );
}

#[test]
fn response_writer_streams_large_bodies_and_maps_reason_phrases() {
    let body = vec![b'x'; RESPONSE_COALESCE_LIMIT + 1];
    let mut response = Vec::new();
    write_response(
        &mut response,
        201,
        reason_phrase(201),
        &[
            ("Connection".to_string(), "keep-alive".to_string()),
            ("Content-Length".to_string(), body.len().to_string()),
        ],
        &body,
    )
    .unwrap();
    assert!(response.starts_with(b"HTTP/1.1 201 Created\r\n"));
    assert!(response.ends_with(&body));
    assert_eq!(
        String::from_utf8_lossy(&response)
            .matches("Content-Length:")
            .count(),
        1
    );
    assert!(!String::from_utf8_lossy(&response).contains("keep-alive"));

    for (status, reason) in [
        (200, "OK"),
        (201, "Created"),
        (204, "No Content"),
        (400, "Bad Request"),
        (401, "Unauthorized"),
        (404, "Not Found"),
        (409, "Conflict"),
        (413, "Content Too Large"),
        (431, "Request Header Fields Too Large"),
        (500, "Internal Server Error"),
        (502, "Bad Gateway"),
        (599, "OK"),
    ] {
        assert_eq!(reason_phrase(status), reason);
    }
}
