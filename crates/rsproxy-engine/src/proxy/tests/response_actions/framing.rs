use super::*;

#[test]
fn response_trailer_actions_set_override_and_remove() {
    let mut trailers = vec![
        ("X-Origin".to_string(), "old".to_string()),
        ("X-Remove".to_string(), "gone".to_string()),
    ];
    let meta = RequestMeta {
        method: "GET".to_string(),
        url: "http://example.com/jobs/42?debug=1".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    };
    let actions = vec![
        resolved(Action::ResTrailer(HeaderOp::Set {
            name: "x-added".to_string(),
            value: Value::inline("${path}"),
        })),
        resolved(Action::ResTrailer(HeaderOp::Set {
            name: "x-origin".to_string(),
            value: Value::inline("new"),
        })),
        resolved(Action::ResTrailer(HeaderOp::Remove {
            name: "x-remove".to_string(),
        })),
    ];

    apply_response_trailer_actions(&mut trailers, &meta, &actions, &test_state()).unwrap();
    assert_eq!(http::header(&trailers, "x-added"), Some("/jobs/42"));
    assert_eq!(http::header(&trailers, "x-origin"), Some("new"));
    assert_eq!(http::header(&trailers, "x-remove"), None);
}

#[test]
fn chunked_response_writer_emits_final_trailers() {
    let mut out = Vec::new();
    let head = http::RawResponseHead {
        version: "HTTP/1.1".to_string(),
        status: 200,
        reason: "OK".to_string(),
        headers: Vec::new(),
    };
    let headers = vec![
        ("Content-Type".to_string(), "text/plain".to_string()),
        ("Transfer-Encoding".to_string(), "chunked".to_string()),
        ("Trailer".to_string(), "x-done".to_string()),
    ];
    let trailers = vec![("x-done".to_string(), "yes".to_string())];

    write_chunked_response(
        &mut out,
        &head,
        &headers,
        b"hello",
        &trailers,
        None,
        ClientPersistence::Close,
    )
    .unwrap();
    let out = String::from_utf8(out).unwrap();
    assert!(out.contains("Transfer-Encoding: chunked\r\n"));
    assert!(out.contains("Trailer: x-done\r\n"));
    assert!(out.ends_with("5\r\nhello\r\n0\r\nx-done: yes\r\n\r\n"));
}

#[test]
fn request_trailers_use_chunked_framing_for_h1_upstreams() {
    let request = RawRequest {
        method: "POST".to_string(),
        target: "/upload".to_string(),
        version: "HTTP/2".to_string(),
        headers: vec![("Content-Length".to_string(), "5".to_string())],
        body: b"hello".to_vec(),
        trailers: vec![("x-checksum".to_string(), "abc".to_string())],
    };
    let mut headers = request.headers.clone();

    prepare_upstream_request_framing(&mut headers, &request);

    assert!(http::header(&headers, "content-length").is_none());
    assert_eq!(http::header(&headers, "transfer-encoding"), Some("chunked"));
    assert_eq!(http::header(&headers, "trailer"), Some("x-checksum"));
    let mut out = Vec::new();
    write_chunked_request(&mut out, &request.body, &request.trailers, None).unwrap();
    assert_eq!(out, b"5\r\nhello\r\n0\r\nx-checksum: abc\r\n\r\n");
}

#[test]
fn decoded_chunked_request_without_trailers_becomes_content_length() {
    let request = RawRequest {
        method: "POST".to_string(),
        target: "/upload".to_string(),
        version: "HTTP/1.1".to_string(),
        headers: vec![("Transfer-Encoding".to_string(), "chunked".to_string())],
        body: b"hello".to_vec(),
        trailers: Vec::new(),
    };
    let mut headers = request.headers.clone();

    prepare_upstream_request_framing(&mut headers, &request);

    assert!(http::header(&headers, "transfer-encoding").is_none());
    assert_eq!(http::header(&headers, "content-length"), Some("5"));
}

#[test]
fn streaming_sse_decodes_chunked_and_captures_frames() {
    let mut upstream = Cursor::new(
        b"6\r\ndata: \r\n7\r\none\r\n\r\n\r\n11\r\nid: 2\r\ndata: two\n\r\n0\r\nx-ignored: yes\r\n\r\n"
            .to_vec(),
    );
    let mut out = Vec::new();
    let headers = vec![
        ("Content-Type".to_string(), "text/event-stream".to_string()),
        ("Transfer-Encoding".to_string(), "chunked".to_string()),
    ];

    let (bytes, body_head, frames) =
        stream_sse_response(&mut out, &mut upstream, &headers, 11, None, |_| {}).unwrap();

    assert_eq!(bytes, out.len() as u64);
    assert_eq!(out, b"data: one\r\n\r\nid: 2\r\ndata: two\n");
    assert_eq!(body_head, b"data: one\r\n".to_vec());
    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].data, b"data: one");
    assert_eq!(frames[1].data, b"id: 2\ndata: two");
}
