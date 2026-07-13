use super::*;

fn collect(encoded: &[u8], headers: Vec<(String, String)>) -> io::Result<Vec<UpstreamBodyFrame>> {
    let mut stream = H1BodyStream::new(
        Cursor::new(encoded),
        "GET",
        200,
        &headers,
        1024,
        8,
        Instant::now(),
    )?;
    let mut frames = Vec::new();
    while let Some(frame) = stream.next_frame() {
        frames.push(frame?);
    }
    Ok(frames)
}

#[test]
fn streams_chunk_data_and_trailers_as_separate_frames() {
    let frames = collect(
        b"5\r\nhello\r\n6\r\n world\r\n0\r\nX-End: yes\r\n\r\n",
        vec![("Transfer-Encoding".to_string(), "chunked".to_string())],
    )
    .unwrap();

    assert!(matches!(&frames[0], UpstreamBodyFrame::Data(data) if data == "hello"));
    assert!(matches!(&frames[1], UpstreamBodyFrame::Data(data) if data == " world"));
    assert!(matches!(
        &frames[2],
        UpstreamBodyFrame::Trailers(trailers)
            if trailers == &[("X-End".to_string(), "yes".to_string())]
    ));
}

#[test]
fn reports_truncated_content_length() {
    let error = collect(
        b"short",
        vec![("Content-Length".to_string(), "10".to_string())],
    )
    .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert!(error.to_string().contains("stage=response_body"));
}

#[test]
fn ignores_body_bytes_for_head_response() {
    let mut stream = H1BodyStream::new(
        Cursor::new(b"unexpected"),
        "HEAD",
        200,
        &[("Content-Length".to_string(), "10".to_string())],
        1024,
        8,
        Instant::now(),
    )
    .unwrap();

    assert!(stream.next_frame().is_none());
}
