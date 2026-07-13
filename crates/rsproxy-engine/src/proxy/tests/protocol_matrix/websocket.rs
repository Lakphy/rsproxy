use super::*;

const SERVER_FIRST: &[u8] = b"server-first";
const CLIENT_TEXT: &[u8] = b"client-message";
const SERVER_ECHO: &[u8] = b"echo:client-message";

#[test]
fn websocket_upgrade_forwards_server_first_and_client_frames_over_real_sockets() {
    let origin = TcpListener::bind("127.0.0.1:0").unwrap();
    let origin_address = origin.local_addr().unwrap();
    let origin_server = thread::spawn(move || {
        let (mut stream, _) = origin.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let head = http::read_request_head(&mut stream, 64 * 1024, 128)
            .unwrap()
            .unwrap();
        assert_eq!(head.request.target, "/socket");
        assert!(is_websocket_request(&head.request.headers));
        stream
            .write_all(
                b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: fixture\r\nX-Origin-WebSocket: yes\r\n\r\n",
            )
            .unwrap();
        stream.write_all(&server_frame(0x1, SERVER_FIRST)).unwrap();
        stream.flush().unwrap();

        let text = read_ws_frame(&mut stream).unwrap().unwrap();
        let close = read_ws_frame(&mut stream).unwrap().unwrap();
        assert_eq!(text.opcode, 0x1);
        assert_eq!(text.payload, CLIENT_TEXT);
        assert_eq!(close.opcode, 0x8);

        stream.write_all(&server_frame(0x1, SERVER_ECHO)).unwrap();
        stream.write_all(&server_frame(0x8, &[])).unwrap();
        stream.flush().unwrap();
    });
    let state = isolated_state(
        "protocol-websocket",
        "127.0.0.1 res.header(x-matrix-websocket: yes) when status(101)",
    );
    let (proxy, proxy_server) = spawn_proxy(state.clone(), 1);
    let mut client = connect_client(proxy);
    client
        .write_all(
            format!(
                "GET http://{origin_address}/socket HTTP/1.1\r\nHost: {origin_address}\r\nUpgrade: websocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n"
            )
            .as_bytes(),
        )
        .unwrap();
    client.flush().unwrap();

    let response = http::read_response_head(&mut client, 64 * 1024, 128).unwrap();
    assert_eq!(response.status, 101);
    assert_eq!(
        http::header(&response.headers, "x-origin-websocket"),
        Some("yes")
    );
    assert_eq!(
        http::header(&response.headers, "x-matrix-websocket"),
        Some("yes")
    );
    let first = read_ws_frame(&mut client).unwrap().unwrap();
    assert_eq!(first.payload, SERVER_FIRST);

    client.write_all(&client_frame(0x1, CLIENT_TEXT)).unwrap();
    client.write_all(&client_frame(0x8, &[])).unwrap();
    client.flush().unwrap();
    let echo = read_ws_frame(&mut client).unwrap().unwrap();
    let close = read_ws_frame(&mut client).unwrap().unwrap();
    assert_eq!(echo.payload, SERVER_ECHO);
    assert_eq!(close.opcode, 0x8);
    drop(client);

    origin_server.join().unwrap();
    proxy_server.join().unwrap();
    let sessions = state.trace.list(2);
    assert_eq!(sessions.len(), 1);
    let session = &sessions[0];
    assert_eq!(session.kind, SessionKind::WebSocket);
    assert_eq!(session.status, Some(101));
    assert!(session.flags.contains(&"websocket".to_string()));
    assert!(session.frames.iter().any(|frame| {
        frame.direction == FrameDirection::ServerToClient && frame.data == SERVER_FIRST
    }));
    assert!(session.frames.iter().any(|frame| {
        frame.direction == FrameDirection::ClientToServer && frame.data == CLIENT_TEXT
    }));
    assert!(session.frames.iter().any(|frame| {
        frame.direction == FrameDirection::ServerToClient && frame.data == SERVER_ECHO
    }));
    let _ = fs::remove_dir_all(&state.config.storage);
}

fn server_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() < 126);
    let mut frame = vec![0x80 | opcode, payload.len() as u8];
    frame.extend_from_slice(payload);
    frame
}

fn client_frame(opcode: u8, payload: &[u8]) -> Vec<u8> {
    assert!(payload.len() < 126);
    let mask = [0x11, 0x22, 0x33, 0x44];
    let mut frame = vec![0x80 | opcode, 0x80 | payload.len() as u8];
    frame.extend_from_slice(&mask);
    frame.extend(
        payload
            .iter()
            .enumerate()
            .map(|(index, byte)| byte ^ mask[index % mask.len()]),
    );
    frame
}
