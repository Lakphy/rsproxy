use super::*;

#[test]
fn h2_frames_are_exposed_as_one_chunked_request_stream() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (sender, receiver) = mpsc::channel(4);
    sender
        .try_send(Ok(DownstreamH2RequestFrame::Data(Bytes::from_static(
            b"abc",
        ))))
        .unwrap();
    sender
        .try_send(Ok(DownstreamH2RequestFrame::Data(Bytes::from_static(
            b"def",
        ))))
        .unwrap();
    sender
        .try_send(Ok(DownstreamH2RequestFrame::Trailers(vec![(
            "x-checksum".to_string(),
            "abcdef".to_string(),
        )])))
        .unwrap();
    drop(sender);
    let mut reader = H2RequestReader::new(receiver, runtime.handle().clone());

    let (body, trailers) = http::read_request_body_all(
        &mut reader,
        http::RequestBodyReader::new(http::RequestBodyFraming::Chunked),
        16 * 1024,
        64,
    )
    .unwrap();

    assert_eq!(body, b"abcdef");
    assert_eq!(
        trailers,
        vec![("x-checksum".to_string(), "abcdef".to_string())]
    );
}

#[test]
fn request_channel_error_is_preserved_as_io_error() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (sender, receiver) = mpsc::channel(1);
    sender
        .try_send(Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "h2 stream reset",
        )))
        .unwrap();
    let mut reader = H2RequestReader::new(receiver, runtime.handle().clone());
    let mut byte = [0u8; 1];

    let error = reader.read(&mut byte).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("h2 stream reset"));
}
