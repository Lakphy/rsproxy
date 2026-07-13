use super::*;

fn collect_output(
    runtime: &tokio::runtime::Runtime,
    mut output: H2BridgeOutput,
) -> (
    DownstreamH2ResponseHead,
    Vec<DownstreamH2ResponseFrame>,
    Option<io::Error>,
) {
    runtime.block_on(async move {
        let head = output.head.await.unwrap().unwrap();
        let mut frames = Vec::new();
        let mut error = None;
        while let Some(frame) = output.body.recv().await {
            match frame {
                Ok(frame) => frames.push(frame),
                Err(seen) => {
                    error = Some(seen);
                    break;
                }
            }
        }
        (head, frames, error)
    })
}

fn collect_head_error(runtime: &tokio::runtime::Runtime, output: H2BridgeOutput) -> io::Error {
    runtime.block_on(async move {
        match output.head.await.unwrap() {
            Ok(head) => panic!("unexpected response head: {head:?}"),
            Err(error) => error,
        }
    })
}

#[test]
fn chunked_response_is_incrementally_decoded_with_trailers() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 16 * 1024, 64, 8);
    writer
        .write_all(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nab",
        )
        .unwrap();
    writer
        .write_all(b"c\r\n3\r\ndef\r\n0\r\nX-Checksum: abcdef\r\n\r\n")
        .unwrap();
    writer.finish().unwrap();

    let (head, frames, error) = collect_output(&runtime, output);

    assert_eq!(head.status, 200);
    assert!(http::header(&head.headers, "transfer-encoding").is_none());
    assert!(http::header(&head.headers, "content-length").is_none());
    assert!(error.is_none());
    let body = frames
        .iter()
        .filter_map(|frame| match frame {
            DownstreamH2ResponseFrame::Data(data) => Some(data.as_ref()),
            DownstreamH2ResponseFrame::Trailers(_) => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(body, b"abcdef");
    assert!(matches!(
        frames.last(),
        Some(DownstreamH2ResponseFrame::Trailers(trailers))
            if trailers == &vec![("X-Checksum".to_string(), "abcdef".to_string())]
    ));
}

#[test]
fn incomplete_chunked_response_reports_body_error_after_head() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 16 * 1024, 64, 8);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n7\r\npartial")
        .unwrap();

    assert!(writer.finish().is_err());
    let (head, frames, error) = collect_output(&runtime, output);

    assert_eq!(head.status, 200);
    assert!(
        matches!(frames.as_slice(), [DownstreamH2ResponseFrame::Data(data)] if data.as_ref() == b"partial")
    );
    assert!(
        error
            .unwrap()
            .to_string()
            .contains("before framing completed")
    );
}

#[test]
fn head_response_discards_pipeline_body_without_error() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("HEAD", 16 * 1024, 64, 8);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nignored")
        .unwrap();
    writer.finish().unwrap();

    let (head, frames, error) = collect_output(&runtime, output);

    assert_eq!(head.status, 200);
    assert_eq!(http::header(&head.headers, "content-length"), Some("7"));
    assert!(frames.is_empty());
    assert!(error.is_none());
}

#[test]
fn no_content_response_discards_pipeline_body_without_error() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 16 * 1024, 64, 8);
    writer
        .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 7\r\n\r\nignored")
        .unwrap();
    writer.finish().unwrap();

    let (head, frames, error) = collect_output(&runtime, output);

    assert_eq!(head.status, 204);
    assert!(frames.is_empty());
    assert!(error.is_none());
}

#[test]
fn close_delimited_response_streams_until_finish() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 16 * 1024, 64, 8);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\r\nfirst")
        .unwrap();
    writer.write_all(b"-second").unwrap();
    writer.flush().unwrap();
    writer.finish().unwrap();

    let (head, frames, error) = collect_output(&runtime, output);
    assert_eq!(head.status, 200);
    assert!(error.is_none());
    let data = frames
        .into_iter()
        .flat_map(|frame| match frame {
            DownstreamH2ResponseFrame::Data(data) => data.to_vec(),
            DownstreamH2ResponseFrame::Trailers(_) => Vec::new(),
        })
        .collect::<Vec<_>>();
    assert_eq!(data, b"first-second");
}

#[test]
fn incomplete_head_and_external_failure_signal_the_waiting_receiver() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    writer.write_all(b"HTTP/1.1 200").unwrap();
    let error = writer.finish().unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
    assert!(error.to_string().contains("before a response head"));
    assert_eq!(
        writer.write(b"ignored").unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    writer.finish().unwrap();
    assert!(
        collect_head_error(&runtime, output)
            .to_string()
            .contains("before a response head")
    );

    let (mut writer, output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    writer.fail_external(&io::Error::new(
        io::ErrorKind::ConnectionReset,
        "upstream reset",
    ));
    let error = collect_head_error(&runtime, output);
    assert_eq!(error.kind(), io::ErrorKind::ConnectionReset);
    assert_eq!(error.to_string(), "upstream reset");
}

#[test]
fn external_failure_after_head_is_forwarded_to_the_body() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\n")
        .unwrap();
    writer.fail_external(&io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "body vanished",
    ));

    let (head, frames, error) = collect_output(&runtime, output);
    assert_eq!(head.status, 200);
    assert!(frames.is_empty());
    assert_eq!(error.unwrap().to_string(), "body vanished");
}

#[test]
fn head_validation_rejects_limits_upgrade_and_invalid_length() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 8, 16, 2);
    let error = writer.write_all(b"HTTP/1.1 200 OK").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("header size limit"));
    assert!(
        collect_head_error(&runtime, output)
            .to_string()
            .contains("header size limit")
    );

    let (mut writer, output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    let error = writer
        .write_all(b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n")
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Unsupported);
    assert!(
        collect_head_error(&runtime, output)
            .to_string()
            .contains("WebSocket over HTTP/2")
    );

    let (mut writer, _output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    let error = writer
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: nope\r\n\r\n")
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(
        error
            .to_string()
            .contains("invalid response content-length")
    );
}

#[test]
fn content_length_rejects_extra_bytes_and_cancelled_consumers() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (mut writer, output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc")
        .unwrap();
    let error = writer.write_all(b"extra").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("bytes written after"));
    let (head, frames, error) = collect_output(&runtime, output);
    assert_eq!(head.status, 200);
    assert!(matches!(
        frames.as_slice(),
        [DownstreamH2ResponseFrame::Data(data)] if data.as_ref() == b"abc"
    ));
    assert!(error.is_none());

    let (mut writer, output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    let H2BridgeOutput { head, body } = output;
    drop(body);
    let error = writer
        .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\n\r\nabc")
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.to_string().contains("body was cancelled"));
    let received = runtime.block_on(head).unwrap().unwrap();
    assert_eq!(received.status, 200);

    let (mut writer, output) = H2ResponseWriter::new("GET", 1024, 16, 2);
    drop(output.head);
    let error = writer.write_all(b"HTTP/1.1 200 OK\r\n\r\n").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    assert!(error.to_string().contains("stream was cancelled"));
}

#[test]
fn chunk_parser_rejects_malformed_sizes_terminators_and_trailer_limits() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    for (payload, message) in [
        (&b"\xff\r\n"[..], "invalid chunk size encoding"),
        (&b"xyz\r\n"[..], "invalid chunk size"),
        (&b"1\r\naXX"[..], "invalid chunk terminator"),
    ] {
        let (mut writer, _output) = H2ResponseWriter::new("GET", 1024, 16, 4);
        writer
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
            .unwrap();
        let error = writer.write_all(payload).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains(message), "{error}");
    }

    let (mut writer, _output) = H2ResponseWriter::new("GET", 64, 16, 4);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
        .unwrap();
    let error = writer.write_all(&[b'f'; 65]).unwrap_err();
    assert!(error.to_string().contains("chunk size line limit"));

    let (mut writer, output) = H2ResponseWriter::new("GET", 128, 16, 4);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n")
        .unwrap();
    writer.finish().unwrap();
    let (_, frames, error) = collect_output(&runtime, output);
    assert!(frames.is_empty());
    assert!(error.is_none());

    let oversized_trailer = format!("X-Long: {}\r\n\r\n", "v".repeat(128));
    for (max_size, max_count, trailer, message) in [
        (
            1024,
            16,
            &b"malformed\r\n\r\n"[..],
            "invalid bridged response trailer",
        ),
        (
            1024,
            16,
            &b"X-Bad: \xff\r\n\r\n"[..],
            "invalid trailer encoding",
        ),
        (
            1024,
            1,
            &b"X-One: 1\r\nX-Two: 2\r\n\r\n"[..],
            "trailer count limit",
        ),
        (128, 16, oversized_trailer.as_bytes(), "trailer size limit"),
    ] {
        let (mut writer, _output) = H2ResponseWriter::new("GET", max_size, max_count, 4);
        writer
            .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n")
            .unwrap();
        let error = writer.write_all(trailer).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }

    let (mut writer, output) = H2ResponseWriter::new("GET", 64, 16, 4);
    writer
        .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n")
        .unwrap();
    let error = writer.write_all(&[b'x'; 69]).unwrap_err();
    assert!(error.to_string().contains("trailer size limit"));
    let (_, _, body_error) = collect_output(&runtime, output);
    assert!(
        body_error
            .unwrap()
            .to_string()
            .contains("trailer size limit")
    );
}
