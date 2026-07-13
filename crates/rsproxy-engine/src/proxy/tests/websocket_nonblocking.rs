use super::*;
use std::collections::VecDeque;

enum ReadStep {
    Data(Vec<u8>),
    Error(io::ErrorKind),
    Eof,
}

struct ScriptedReader {
    steps: VecDeque<ReadStep>,
}

impl ScriptedReader {
    fn new(steps: impl IntoIterator<Item = ReadStep>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
        }
    }
}

impl Read for ScriptedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.steps.pop_front().unwrap_or(ReadStep::Eof) {
            ReadStep::Data(mut data) => {
                let used = data.len().min(buf.len());
                buf[..used].copy_from_slice(&data[..used]);
                if used < data.len() {
                    data.drain(..used);
                    self.steps.push_front(ReadStep::Data(data));
                }
                Ok(used)
            }
            ReadStep::Error(kind) => Err(io::Error::new(kind, "scripted read")),
            ReadStep::Eof => Ok(0),
        }
    }
}

enum WriteStep {
    All,
    Bytes(usize),
    Zero,
    Error(io::ErrorKind),
}

struct ScriptedWriter {
    steps: VecDeque<WriteStep>,
    output: Vec<u8>,
    flush_error: Option<io::ErrorKind>,
}

impl ScriptedWriter {
    fn new(steps: impl IntoIterator<Item = WriteStep>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            output: Vec::new(),
            flush_error: None,
        }
    }
}

impl Write for ScriptedWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.steps.pop_front().unwrap_or(WriteStep::All) {
            WriteStep::All => {
                self.output.extend_from_slice(buf);
                Ok(buf.len())
            }
            WriteStep::Bytes(limit) => {
                let used = limit.min(buf.len());
                self.output.extend_from_slice(&buf[..used]);
                Ok(used)
            }
            WriteStep::Zero => Ok(0),
            WriteStep::Error(kind) => Err(io::Error::new(kind, "scripted write")),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.flush_error {
            Some(kind) => Err(io::Error::new(kind, "scripted flush")),
            None => Ok(()),
        }
    }
}

struct MemoryWs {
    input: ScriptedReader,
    output: Vec<u8>,
    nonblocking_calls: Vec<bool>,
    shutdowns: Vec<Shutdown>,
    fail_initial_nonblocking: bool,
    fail_restore_nonblocking: bool,
}

impl MemoryWs {
    fn new(input: Vec<u8>) -> Self {
        Self {
            input: ScriptedReader::new([ReadStep::Data(input), ReadStep::Eof]),
            output: Vec::new(),
            nonblocking_calls: Vec::new(),
            shutdowns: Vec::new(),
            fail_initial_nonblocking: false,
            fail_restore_nonblocking: false,
        }
    }
}

impl Read for MemoryWs {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for MemoryWs {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl WsIo for MemoryWs {
    fn set_ws_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.nonblocking_calls.push(nonblocking);
        if (nonblocking && self.fail_initial_nonblocking)
            || (!nonblocking && self.fail_restore_nonblocking)
        {
            Err(io::Error::other("scripted nonblocking failure"))
        } else {
            Ok(())
        }
    }

    fn shutdown_ws(&mut self, how: Shutdown) -> io::Result<()> {
        self.shutdowns.push(how);
        Ok(())
    }

    fn set_request_read_timeout(&mut self, _timeout: Option<Duration>) -> io::Result<()> {
        Ok(())
    }
}

fn websocket_pair() -> (UpstreamStream, thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let client = TcpStream::connect(address).unwrap();
    let (mut peer, _) = listener.accept().unwrap();
    let handle = thread::spawn(move || {
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let mut request = Vec::new();
        peer.read_to_end(&mut request).unwrap();
        peer.write_all(b"\x81\x02ok\x88\x00").unwrap();
        peer.shutdown(Shutdown::Write).unwrap();
        request
    });
    (UpstreamStream::Tcp(client), handle)
}

#[test]
fn nonblocking_tunnel_forwards_frames_and_restores_blocking_mode() {
    let request = b"\x81\x02hi\x88\x00".to_vec();
    let mut client = MemoryWs::new(request.clone());
    let (mut upstream, peer) = websocket_pair();

    let (request_bytes, response_bytes, frames) =
        websocket_tunnel(&mut client, None, &mut upstream, 32).unwrap();

    assert_eq!(peer.join().unwrap(), request);
    assert_eq!(client.output, b"\x81\x02ok\x88\x00");
    assert_eq!(request_bytes, 6);
    assert_eq!(response_bytes, 6);
    assert_eq!(frames.len(), 4);
    assert_eq!(frames[0].direction, FrameDirection::ClientToServer);
    assert_eq!(frames[2].direction, FrameDirection::ServerToClient);
    assert_eq!(client.nonblocking_calls, [true, false]);
    assert_eq!(client.shutdowns, [Shutdown::Write]);
}

#[test]
fn nonblocking_tunnel_reports_setup_and_restore_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let upstream_tcp = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (_peer, _) = listener.accept().unwrap();
    let mut upstream = UpstreamStream::Tcp(upstream_tcp);
    let mut setup_failure = MemoryWs::new(Vec::new());
    setup_failure.fail_initial_nonblocking = true;
    let error = websocket_tunnel(&mut setup_failure, None, &mut upstream, 8).unwrap_err();
    assert!(error.to_string().contains("nonblocking failure"));

    let mut restore_failure = MemoryWs::new(b"\x88\x00".to_vec());
    restore_failure.fail_restore_nonblocking = true;
    let (mut upstream, peer) = websocket_pair();
    let error = websocket_tunnel(&mut restore_failure, None, &mut upstream, 8).unwrap_err();
    assert!(error.to_string().contains("nonblocking failure"));
    assert_eq!(peer.join().unwrap(), b"\x88\x00");
}

#[test]
fn nonblocking_reader_handles_full_buffers_would_block_eof_and_errors() {
    let payload = vec![b'x'; 8188];
    let mut frame = vec![0x82, 0x7e, 0x1f, 0xfc];
    frame.extend_from_slice(&payload);
    assert_eq!(frame.len(), 8192);
    let mut reader = ScriptedReader::new([
        ReadStep::Data(frame),
        ReadStep::Error(io::ErrorKind::WouldBlock),
    ]);
    let (bytes, frames, closed) =
        read_ws_frames_nonblocking(&mut reader, &mut WsFrameDecoder::default()).unwrap();
    assert_eq!(bytes, 8192);
    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].payload.len(), 8188);
    assert!(!closed);

    let mut eof = ScriptedReader::new([ReadStep::Eof]);
    let result = read_ws_frames_nonblocking(&mut eof, &mut WsFrameDecoder::default()).unwrap();
    assert_eq!((result.0, result.1.len(), result.2), (0, 0, true));

    let mut failed = ScriptedReader::new([ReadStep::Error(io::ErrorKind::InvalidData)]);
    let error = match read_ws_frames_nonblocking(&mut failed, &mut WsFrameDecoder::default()) {
        Err(error) => error,
        Ok(_) => panic!("scripted read error was ignored"),
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn nonblocking_writer_handles_partial_backpressure_and_end_errors() {
    let mut pending = b"abcdef".to_vec();
    let mut writer = ScriptedWriter::new([WriteStep::Bytes(2), WriteStep::All]);
    assert_eq!(
        flush_pending_nonblocking(&mut writer, &mut pending).unwrap(),
        6
    );
    assert!(pending.is_empty());
    assert_eq!(writer.output, b"abcdef");

    for step in [
        WriteStep::Zero,
        WriteStep::Error(io::ErrorKind::WouldBlock),
        WriteStep::Error(io::ErrorKind::BrokenPipe),
    ] {
        let mut pending = b"data".to_vec();
        let mut writer = ScriptedWriter::new([step]);
        assert_eq!(
            flush_pending_nonblocking(&mut writer, &mut pending).unwrap(),
            0
        );
        assert_eq!(pending, b"data");
    }

    let mut pending = b"data".to_vec();
    let mut writer = ScriptedWriter::new([WriteStep::Error(io::ErrorKind::PermissionDenied)]);
    let error = flush_pending_nonblocking(&mut writer, &mut pending).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn nonblocking_writer_classifies_flush_failures() {
    for kind in [io::ErrorKind::WouldBlock, io::ErrorKind::BrokenPipe] {
        let mut pending = b"data".to_vec();
        let mut writer = ScriptedWriter::new([WriteStep::All]);
        writer.flush_error = Some(kind);
        assert_eq!(
            flush_pending_nonblocking(&mut writer, &mut pending).unwrap(),
            4
        );
    }

    let mut pending = b"data".to_vec();
    let mut writer = ScriptedWriter::new([WriteStep::All]);
    writer.flush_error = Some(io::ErrorKind::InvalidData);
    let error = flush_pending_nonblocking(&mut writer, &mut pending).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}
