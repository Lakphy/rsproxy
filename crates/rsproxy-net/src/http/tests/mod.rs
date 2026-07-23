use super::*;
use std::io;

#[test]
fn request_header_count_limit_is_enforced() {
    let raw = b"GET / HTTP/1.1\r\nHost: example.test\r\nX-One: 1\r\n\r\n";
    let err = read_request(&mut io::Cursor::new(raw), 1024, 1).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "header count limit exceeded (limit 1)");
}

#[test]
fn response_header_count_limit_is_enforced() {
    let raw = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nX-One: 1\r\n\r\n";
    let err = read_response_head(&mut io::Cursor::new(raw), 1024, 1).unwrap_err();

    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    assert_eq!(err.to_string(), "header count limit exceeded (limit 1)");
}

#[test]
fn chunked_request_body_and_trailers_are_decoded() {
    let raw = b"POST /upload HTTP/1.1\r\nHost: example.test\r\nTransfer-Encoding: chunked\r\nTrailer: X-Checksum\r\n\r\n4;ext=yes\r\nWiki\r\n5\r\npedia\r\n0\r\nX-Checksum: abc123\r\n\r\n";

    let request = read_request(&mut io::Cursor::new(raw), 4096, 8)
        .unwrap()
        .unwrap();

    assert_eq!(request.body, b"Wikipedia");
    assert_eq!(
        request.trailers,
        vec![("X-Checksum".to_string(), "abc123".to_string())]
    );
}

#[test]
fn request_head_leaves_the_body_for_incremental_consumption() {
    let raw = b"POST /upload HTTP/1.1\r\nHost: example.test\r\nContent-Length: 6\r\n\r\nabcdef";
    let mut cursor = io::Cursor::new(raw);

    let head = read_request_head(&mut cursor, 4096, 8).unwrap().unwrap();

    assert_eq!(head.request.target, "/upload");
    assert_eq!(head.body, RequestBodyFraming::ContentLength(6));
    assert!(head.body.has_body());
    assert_eq!(&raw[cursor.position() as usize..], b"abcdef");
}

#[test]
fn known_oversized_request_body_can_continue_from_the_same_reader() {
    let raw = b"POST /upload HTTP/1.1\r\nContent-Length: 6\r\n\r\nabcdef";
    let mut cursor = io::Cursor::new(raw);
    let head = read_request_head(&mut cursor, 4096, 8).unwrap().unwrap();

    let BoundedRequestBody::Overflow { prefix, reader } =
        read_request_body_bounded(&mut cursor, head.body, 3, 4096, 8).unwrap()
    else {
        panic!("expected body overflow");
    };
    assert!(prefix.is_empty());
    assert_eq!(reader.framing(), RequestBodyFraming::ContentLength(6));
    let (remaining, trailers) = read_request_body_all(&mut cursor, reader, 4096, 8).unwrap();
    assert_eq!(remaining, b"abcdef");
    assert!(trailers.is_empty());
}

#[test]
fn chunked_overflow_preserves_prefix_remaining_data_and_trailers() {
    let raw = b"POST /upload HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n3\r\ndef\r\n0\r\nX-End: yes\r\n\r\n";
    let mut cursor = io::Cursor::new(raw);
    let head = read_request_head(&mut cursor, 4096, 8).unwrap().unwrap();

    let BoundedRequestBody::Overflow { mut prefix, reader } =
        read_request_body_bounded(&mut cursor, head.body, 4, 4096, 8).unwrap()
    else {
        panic!("expected body overflow");
    };
    assert_eq!(prefix, b"abcde");
    assert_eq!(reader.framing(), RequestBodyFraming::Chunked);
    let (remaining, trailers) = read_request_body_all(&mut cursor, reader, 4096, 8).unwrap();
    prefix.extend_from_slice(&remaining);
    assert_eq!(prefix, b"abcdef");
    assert_eq!(trailers, vec![("X-End".to_string(), "yes".to_string())]);
}

#[test]
fn request_rejects_content_length_transfer_encoding_ambiguity() {
    let raw =
        b"POST / HTTP/1.1\r\nContent-Length: 4\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n";

    let error = read_request(&mut io::Cursor::new(raw), 4096, 8).unwrap_err();

    assert!(error.to_string().contains("both Content-Length"));
}

#[test]
fn request_rejects_conflicting_content_lengths() {
    let raw = b"POST / HTTP/1.1\r\nContent-Length: 4\r\nContent-Length: 5\r\n\r\nhello";

    let error = read_request(&mut io::Cursor::new(raw), 4096, 8).unwrap_err();

    assert_eq!(error.to_string(), "conflicting Content-Length headers");
}

#[test]
fn chunked_request_trailer_count_is_limited() {
    let raw =
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nX-One: 1\r\nX-Two: 2\r\n\r\n";

    let error = read_request(&mut io::Cursor::new(raw), 4096, 1).unwrap_err();

    assert_eq!(error.to_string(), "trailer count limit exceeded (limit 1)");
}

#[test]
fn chunked_request_rejects_framing_trailers() {
    let raw =
        b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n0\r\nContent-Length: 9\r\n\r\n";

    let error = read_request(&mut io::Cursor::new(raw), 4096, 8).unwrap_err();

    assert_eq!(
        error.to_string(),
        "forbidden request trailer `Content-Length`"
    );
}

#[test]
fn response_writers_emit_one_selected_connection_header() {
    let headers = vec![
        ("Connection".to_string(), "close".to_string()),
        ("X-Test".to_string(), "yes".to_string()),
    ];
    let mut response = Vec::new();
    write_response_with_connection(&mut response, 200, "OK", &headers, b"body", true).unwrap();
    let response = String::from_utf8(response).unwrap();
    assert!(response.contains("X-Test: yes\r\n"));
    assert!(response.contains("Connection: keep-alive\r\n"));
    assert!(!response.contains("Connection: close\r\n"));
    assert_eq!(response.matches("Connection:").count(), 1);

    let mut head = Vec::new();
    write_response_head_with_connection(
        &mut head,
        &RawResponseHead {
            version: "HTTP/1.1".to_string(),
            status: 204,
            reason: "No Content".to_string(),
            headers: Vec::new(),
        },
        &headers,
        false,
    )
    .unwrap();
    let head = String::from_utf8(head).unwrap();
    assert!(head.ends_with("Connection: close\r\n\r\n"));
    assert_eq!(head.matches("Connection:").count(), 1);

    let mut http10 = Vec::new();
    write_response_with_version_and_connection(
        &mut http10,
        "HTTP/1.0",
        200,
        "OK",
        &[],
        b"legacy",
        false,
    )
    .unwrap();
    assert!(
        String::from_utf8(http10)
            .unwrap()
            .starts_with("HTTP/1.0 200 OK\r\n")
    );
}

#[test]
fn complete_response_writer_owns_framing() {
    let headers = vec![
        ("Content-Length".to_string(), "0".to_string()),
        ("Transfer-Encoding".to_string(), "chunked".to_string()),
        ("Trailer".to_string(), "X-End".to_string()),
        ("X-Test".to_string(), "yes".to_string()),
    ];
    let mut response = Vec::new();

    write_response(&mut response, 200, "OK", &headers, b"actual").unwrap();

    let response = String::from_utf8(response).unwrap();
    assert!(response.contains("Content-Length: 6\r\n"));
    assert_eq!(response.matches("Content-Length:").count(), 1);
    assert!(!response.contains("Transfer-Encoding:"));
    assert!(!response.contains("Trailer:"));
    assert!(response.ends_with("\r\n\r\nactual"));
}

#[test]
fn response_writers_reject_injection_before_writing() {
    for headers in [
        vec![("X-Test\r\nInjected".to_string(), "yes".to_string())],
        vec![("X-Test".to_string(), "yes\r\nInjected: true".to_string())],
    ] {
        let mut response = Vec::new();
        let error = write_response(&mut response, 200, "OK", &headers, b"").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(response.is_empty());
    }

    let mut response = Vec::new();
    let error = write_response_with_version_and_connection(
        &mut response,
        "HTTP/1.1\r\nInjected: true",
        200,
        "OK",
        &[],
        b"",
        false,
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(response.is_empty());

    let mut response = Vec::new();
    let error = write_response(&mut response, 200, "OK\r\nInjected: true", &[], b"").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(response.is_empty());
}

#[test]
fn bodyless_statuses_reject_content_and_omit_framing() {
    for status in [100, 204, 304] {
        let mut response = Vec::new();
        let error =
            write_response(&mut response, status, reason_phrase(status), &[], b"body").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(response.is_empty());

        write_response(&mut response, status, reason_phrase(status), &[], b"").unwrap();
        let response = String::from_utf8(response).unwrap();
        assert!(!response.contains("Content-Length:"));
        assert!(!response.contains("Transfer-Encoding:"));
    }
    assert!(!status_can_send_content(204));
    assert!(!response_can_send_content("HEAD", 200));
    assert!(response_can_send_content("GET", 200));
    assert!(response_has_framed_body("GET", 205));
    assert!(!response_can_send_content("GET", 205));

    let mut reset = Vec::new();
    write_response(&mut reset, 205, reason_phrase(205), &[], b"").unwrap();
    let reset = String::from_utf8(reset).unwrap();
    assert!(reset.contains("Content-Length: 0\r\n"));
}

#[test]
fn streaming_response_head_rejects_ambiguous_framing() {
    let head = RawResponseHead {
        version: "HTTP/1.1".to_string(),
        status: 200,
        reason: "OK".to_string(),
        headers: Vec::new(),
    };
    let headers = vec![
        ("Content-Length".to_string(), "4".to_string()),
        ("Transfer-Encoding".to_string(), "chunked".to_string()),
    ];
    let mut response = Vec::new();

    let error = write_response_head(&mut response, &head, &headers).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(response.is_empty());
}

#[test]
fn streaming_response_head_normalizes_content_length_and_bodyless_framing() {
    let mut response = Vec::new();
    let head = RawResponseHead {
        version: "HTTP/1.1".to_string(),
        status: 200,
        reason: "OK".to_string(),
        headers: Vec::new(),
    };
    write_response_head(
        &mut response,
        &head,
        &[
            ("Content-Length".to_string(), "04".to_string()),
            ("content-length".to_string(), "4, 4".to_string()),
        ],
    )
    .unwrap();
    let response = String::from_utf8(response).unwrap();
    assert_eq!(response.matches("Content-Length:").count(), 1);
    assert!(response.contains("Content-Length: 4\r\n"));

    for status in [100, 204, 304] {
        let mut response = Vec::new();
        let head = RawResponseHead {
            version: "HTTP/1.1".to_string(),
            status,
            reason: reason_phrase(status).to_string(),
            headers: Vec::new(),
        };
        write_response_head(
            &mut response,
            &head,
            &[("Content-Length".to_string(), "9".to_string())],
        )
        .unwrap();
        assert!(
            !String::from_utf8(response)
                .unwrap()
                .contains("Content-Length:")
        );
    }
}
