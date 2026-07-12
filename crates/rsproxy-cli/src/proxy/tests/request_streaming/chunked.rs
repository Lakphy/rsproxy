use super::*;

#[test]
fn chunked_upload_streams_decoded_data_and_preserves_trailers() {
    let (origin, requests, origin_worker) = spawn_origin(1, |_, request| {
        (
            Vec::new(),
            format!(
                "body={};trailer={}",
                String::from_utf8_lossy(&request.body),
                http::header(&request.trailers, "x-upload-end").unwrap_or("missing")
            )
            .into_bytes(),
        )
    });
    let mut state = test_state();
    state.config.trace_body_limit = 4;
    state.config.body_buffer_limit = 4;
    let (proxy, proxy_worker) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);

    write!(
        client,
        "POST http://{origin}/chunked HTTP/1.1\r\nHost: {origin}\r\nTransfer-Encoding: chunked\r\nTrailer: X-Upload-End\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    client
        .write_all(b"3\r\nabc\r\n3\r\ndef\r\n0\r\nX-Upload-End: done\r\n\r\n")
        .unwrap();
    client.flush().unwrap();
    let (_, body) = read_response(&mut client);
    assert_eq!(body.body, b"body=abcdef;trailer=done");
    drop(client);

    proxy_worker.join().unwrap();
    origin_worker.join().unwrap();
    let request = requests.recv().unwrap();
    assert_eq!(request.body, b"abcdef");
    assert_eq!(
        http::header(&request.trailers, "x-upload-end"),
        Some("done")
    );
    let session = state.trace.list(1).pop().unwrap();
    assert_eq!(session.request_bytes, 6);
    assert_eq!(session.req_body_head, b"abcd");
    assert_eq!(
        http::header(&session.req_trailers, "x-upload-end"),
        Some("done")
    );
    assert!(session.flags.contains(&"request-streamed".to_string()));
    assert!(session.flags.contains(&"req-trailers".to_string()));
}
