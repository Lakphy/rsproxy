use super::*;

struct ErrorAfterData {
    data: Cursor<Vec<u8>>,
    kind: io::ErrorKind,
    message: &'static str,
}

impl ErrorAfterData {
    fn new(data: &[u8], kind: io::ErrorKind, message: &'static str) -> Self {
        Self {
            data: Cursor::new(data.to_vec()),
            kind,
            message,
        }
    }
}

impl Read for ErrorAfterData {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.data.position() < self.data.get_ref().len() as u64 {
            self.data.read(buffer)
        } else {
            Err(io::Error::new(self.kind, self.message))
        }
    }
}

struct FailingWriter(io::ErrorKind);

impl Write for FailingWriter {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(self.0, "scripted write failure"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn close_delimited_response_handles_eof_tls_shutdown_and_other_errors() {
    let response = read_response_body(&mut Cursor::new(b"close-delimited"), &[]).unwrap();
    assert_eq!(response.body, b"close-delimited");
    assert!(response.trailers.is_empty());

    let mut missing_notify = ErrorAfterData::new(
        b"tls body",
        io::ErrorKind::UnexpectedEof,
        "peer closed without close_notify",
    );
    let response = read_response_body(&mut missing_notify, &[]).unwrap();
    assert_eq!(response.body, b"tls body");

    let mut failed = ErrorAfterData::new(b"partial", io::ErrorKind::Other, "origin failed");
    let error = read_response_body(&mut failed, &[]).err().unwrap();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert_eq!(error.to_string(), "origin failed");

    let mut short = Cursor::new(b"tiny");
    let error = read_response_body(
        &mut short,
        &[("content-length".to_string(), "8".to_string())],
    )
    .err()
    .unwrap();
    assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
}

#[test]
fn sse_streaming_handles_close_delimited_capture_and_frame_limit() {
    let mut output = Vec::new();
    let mut observed = Vec::new();
    let mut input = Cursor::new(b"data: one\r\n\r\ndata: two\r\rdata: tail".to_vec());
    let (bytes, head, frames) =
        stream_sse_response(&mut output, &mut input, &[], 12, None, |data| {
            observed.extend_from_slice(data)
        })
        .unwrap();
    assert_eq!(bytes, output.len() as u64);
    assert_eq!(output, observed);
    assert_eq!(head, output[..12]);
    assert_eq!(frames.len(), 3);
    assert_eq!(frames[0].data, b"data: one");
    assert_eq!(frames[2].data, b"data: tail");

    let input = (0..513)
        .map(|index| format!("data: {index}\n\n"))
        .collect::<String>();
    let mut output = Vec::new();
    let (_, _, frames) =
        stream_sse_response(&mut output, &mut Cursor::new(input), &[], 0, None, |_| {}).unwrap();
    assert_eq!(frames.len(), 512);
}

#[test]
fn sse_streaming_reports_malformed_chunks_and_stops_on_disconnect() {
    let headers = [("transfer-encoding".to_string(), "gzip, Chunked".to_string())];

    let error = stream_sse_response(
        &mut Vec::new(),
        &mut Cursor::new(b"xyz\r\n"),
        &headers,
        32,
        None,
        |_| {},
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("invalid chunk size"));

    let error = stream_sse_response(
        &mut Vec::new(),
        &mut Cursor::new(b"3\r\nabcXX"),
        &headers,
        32,
        None,
        |_| {},
    )
    .unwrap_err();
    assert!(error.to_string().contains("trailing CRLF"));

    let (bytes, head, frames) = stream_sse_response(
        &mut FailingWriter(io::ErrorKind::BrokenPipe),
        &mut Cursor::new(b"3\r\nabc\r\n0\r\n\r\n"),
        &headers,
        32,
        None,
        |_| panic!("disconnected payload must not be observed"),
    )
    .unwrap();
    assert_eq!(bytes, 0);
    assert!(head.is_empty());
    assert!(frames.is_empty());

    let fixed_headers = [("content-length".to_string(), "3".to_string())];
    let result = stream_sse_response(
        &mut FailingWriter(io::ErrorKind::ConnectionReset),
        &mut Cursor::new(b"abc"),
        &fixed_headers,
        32,
        None,
        |_| panic!("disconnected payload must not be observed"),
    )
    .unwrap();
    assert_eq!(result.0, 0);

    let error = write_streaming_payload(
        &mut FailingWriter(io::ErrorKind::InvalidInput),
        b"payload",
        &mut ThrottlePacer::new(None),
    )
    .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(write_streaming_payload(&mut Vec::new(), b"", &mut ThrottlePacer::new(None)).unwrap());
}

#[test]
fn chunked_body_and_line_parsing_reject_malformed_boundaries() {
    let error = read_chunked_body(&mut Cursor::new(b"3\r\nabcXX"))
        .err()
        .unwrap();
    assert!(error.to_string().contains("trailing CRLF"));

    let error = read_chunked_body(&mut Cursor::new(b"nope\r\n"))
        .err()
        .unwrap();
    assert!(error.to_string().contains("invalid chunk size"));

    let response = read_chunked_body(&mut Cursor::new(
        b"1;extension=yes\r\nx\r\n0\r\nMalformed\r\n : ignored\r\nX-End: yes\r\n\r\n",
    ))
    .unwrap();
    assert_eq!(response.body, b"x");
    assert_eq!(
        response.trailers,
        vec![("X-End".to_string(), "yes".to_string())]
    );

    let oversized = format!("{}\r\n", "x".repeat(8193));
    let error = read_crlf_line(&mut Cursor::new(oversized)).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("too large"));

    assert!(tls_close_notify_missing(&io::Error::new(
        io::ErrorKind::InvalidData,
        "peer sent no close_notify",
    )));
    assert!(!tls_close_notify_missing(&io::Error::new(
        io::ErrorKind::UnexpectedEof,
        "plain EOF",
    )));
    assert!(!has_chunked_transfer_encoding(&[(
        "transfer-encoding".to_string(),
        "gzip".to_string(),
    )]));
}
