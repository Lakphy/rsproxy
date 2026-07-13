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
