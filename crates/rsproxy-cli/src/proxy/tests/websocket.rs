use super::*;

#[test]
fn websocket_trace_preserves_fragmentation_metadata() {
    let mut frames = Vec::new();
    let mut state = WsTraceState::default();
    record_ws_frame(
        &mut frames,
        FrameDirection::ClientToServer,
        &WsFrame {
            raw: vec![0x01, 0x03, b'h', b'e', b'l'],
            payload: b"hel".to_vec(),
            opcode: 0x1,
            fin: false,
        },
        64,
        &mut state,
    );
    record_ws_frame(
        &mut frames,
        FrameDirection::ClientToServer,
        &WsFrame {
            raw: vec![0x80, 0x02, b'l', b'o'],
            payload: b"lo".to_vec(),
            opcode: 0x0,
            fin: true,
        },
        64,
        &mut state,
    );

    assert_eq!(frames.len(), 2);
    assert_eq!(frames[0].opcode, "text");
    assert!(!frames[0].fin);
    assert_eq!(frames[0].data_encoding, FrameDataEncoding::Utf8);
    assert_eq!(frames[0].data, b"hel");
    assert_eq!(frames[1].opcode, "continuation");
    assert!(frames[1].fin);
    assert_eq!(frames[1].data_encoding, FrameDataEncoding::Utf8);
    assert_eq!(frames[1].data, b"lo");
}

#[test]
fn websocket_trace_marks_binary_preview_and_control_opcodes() {
    let mut frames = Vec::new();
    let mut state = WsTraceState::default();
    record_ws_frame(
        &mut frames,
        FrameDirection::ServerToClient,
        &WsFrame {
            raw: vec![0x82, 0x05, 0, 1, 2, 3, 4],
            payload: vec![0, 1, 2, 3, 4],
            opcode: 0x2,
            fin: true,
        },
        3,
        &mut state,
    );
    record_ws_frame(
        &mut frames,
        FrameDirection::ServerToClient,
        &WsFrame {
            raw: vec![0x89, 0x04, b'p', b'i', b'n', b'g'],
            payload: b"ping".to_vec(),
            opcode: 0x9,
            fin: true,
        },
        64,
        &mut state,
    );

    assert_eq!(frames[0].opcode, "binary");
    assert_eq!(frames[0].payload_len, 5);
    assert_eq!(frames[0].data_encoding, FrameDataEncoding::Hex);
    assert_eq!(frames[0].data, vec![0, 1, 2]);
    assert!(frames[0].truncated);
    assert_eq!(frames[1].opcode, "ping");
    assert_eq!(frames[1].data_encoding, FrameDataEncoding::Utf8);
    assert_eq!(frames[1].data, b"ping");
}

#[test]
fn websocket_reader_unmasks_client_frames_and_reads_fin() {
    let payload = b"hi";
    let mask = [1u8, 2, 3, 4];
    let mut raw = vec![0x81, 0x80 | payload.len() as u8];
    raw.extend_from_slice(&mask);
    raw.extend(
        payload
            .iter()
            .enumerate()
            .map(|(idx, byte)| byte ^ mask[idx % 4]),
    );

    let frame = read_ws_frame(&mut raw.as_slice()).unwrap().unwrap();

    assert!(frame.fin);
    assert_eq!(frame.opcode, 0x1);
    assert_eq!(frame.payload, payload);
}

#[test]
fn websocket_decoder_waits_for_complete_split_frame() {
    let mut decoder = WsFrameDecoder::default();

    assert!(decoder.push(&[0x81]).unwrap().is_empty());
    assert!(decoder.push(&[0x05, b'h']).unwrap().is_empty());
    let frames = decoder.push(b"ello").unwrap();

    assert_eq!(frames.len(), 1);
    assert_eq!(frames[0].opcode, 0x1);
    assert!(frames[0].fin);
    assert_eq!(frames[0].payload, b"hello");
    assert_eq!(frames[0].raw, b"\x81\x05hello");
}

#[test]
fn websocket_trace_caps_records_but_keeps_fragment_state() {
    let mut frames = (0..512)
        .map(|_| FrameRecord {
            direction: FrameDirection::ClientToServer,
            at_ms: 0,
            opcode: "text".to_string(),
            fin: true,
            payload_len: 0,
            data_encoding: FrameDataEncoding::Utf8,
            data: Vec::new(),
            truncated: false,
        })
        .collect::<Vec<_>>();
    let mut state = WsTraceState::default();
    record_ws_frame(
        &mut frames,
        FrameDirection::ClientToServer,
        &WsFrame {
            raw: b"\x01\x01x".to_vec(),
            payload: b"x".to_vec(),
            opcode: 0x1,
            fin: false,
        },
        8,
        &mut state,
    );

    assert_eq!(frames.len(), 512);
    assert_eq!(state.fragmented_opcode, Some(0x1));
    assert_eq!(ws_opcode_name(0xA), "pong");
    assert_eq!(ws_opcode_name(0x3), "unknown");
    assert_eq!(ws_frame_encoding(0x8, &[0xff]), FrameDataEncoding::Hex);
}

#[test]
fn websocket_reader_handles_eof_io_errors_and_extended_lengths() {
    struct FailingReader;

    impl Read for FailingReader {
        fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        }
    }

    assert!(read_ws_frame(&mut [].as_slice()).unwrap().is_none());
    let error = match read_ws_frame(&mut FailingReader) {
        Err(error) => error,
        Ok(_) => panic!("scripted websocket read error was ignored"),
    };
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);

    let payload126 = vec![b'x'; 126];
    let mut encoded126 = vec![0x82, 126, 0, 126];
    encoded126.extend_from_slice(&payload126);
    let frame = read_ws_frame(&mut encoded126.as_slice()).unwrap().unwrap();
    assert_eq!(frame.payload, payload126);
    assert_eq!(frame.raw, encoded126);

    let mut encoded127 = vec![0x82, 127];
    encoded127.extend_from_slice(&3u64.to_be_bytes());
    encoded127.extend_from_slice(b"abc");
    let frame = read_ws_frame(&mut encoded127.as_slice()).unwrap().unwrap();
    assert_eq!(frame.payload, b"abc");

    let mut oversized = vec![0x82, 127];
    oversized.extend_from_slice(&(16 * 1024 * 1024 + 1u64).to_be_bytes());
    let error = match read_ws_frame(&mut oversized.as_slice()) {
        Err(error) => error,
        Ok(_) => panic!("oversized websocket frame was accepted"),
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
}

#[test]
fn websocket_prefix_parser_validates_extended_and_masked_frames() {
    assert!(parse_ws_frame_prefix(&[0x82, 126]).unwrap().is_none());
    assert!(parse_ws_frame_prefix(&[0x82, 127, 0]).unwrap().is_none());

    let mut encoded127 = vec![0x82, 127];
    encoded127.extend_from_slice(&3u64.to_be_bytes());
    encoded127.extend_from_slice(b"abc");
    let (frame, used) = parse_ws_frame_prefix(&encoded127).unwrap().unwrap();
    assert_eq!(frame.payload, b"abc");
    assert_eq!(used, encoded127.len());

    let mut oversized = vec![0x82, 127];
    oversized.extend_from_slice(&(16 * 1024 * 1024 + 1u64).to_be_bytes());
    let error = match parse_ws_frame_prefix(&oversized) {
        Err(error) => error,
        Ok(_) => panic!("oversized websocket frame was accepted"),
    };
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    assert!(
        parse_ws_frame_prefix(&[0x81, 0x80 | 2, 1, 2, 3])
            .unwrap()
            .is_none()
    );
    let mask = [1u8, 2, 3, 4];
    let mut masked = vec![0x81, 0x80 | 2];
    masked.extend_from_slice(&mask);
    masked.extend([b'h' ^ mask[0], b'i' ^ mask[1]]);
    let (frame, used) = parse_ws_frame_prefix(&masked).unwrap().unwrap();
    assert_eq!(frame.payload, b"hi");
    assert_eq!(frame.raw, masked);
    assert_eq!(used, masked.len());
}
