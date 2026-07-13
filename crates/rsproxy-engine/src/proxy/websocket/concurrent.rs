use super::*;

pub(super) fn websocket_tunnel_concurrent<W: Write + Send>(
    client: &mut W,
    client_reader: TcpStream,
    upstream: &mut UpstreamStream,
    upstream_reader: TcpStream,
    trace_limit: usize,
) -> io::Result<(u64, u64, Vec<FrameRecord>)> {
    let client_shutdown = client_reader.try_clone().ok();
    let upstream_shutdown = upstream_reader.try_clone().ok();
    let frames = Arc::new(Mutex::new(Vec::new()));
    let (c2s, s2c) = thread::scope(|scope| {
        let c2s_frames = Arc::clone(&frames);
        let c2s = scope.spawn(move || {
            let mut reader = client_reader;
            let result = websocket_copy_frames_shared(
                &mut reader,
                upstream,
                FrameDirection::ClientToServer,
                trace_limit,
                &c2s_frames,
            );
            if let Some(stream) = upstream_shutdown {
                let _ = stream.shutdown(Shutdown::Write);
            }
            result
        });
        let s2c_frames = Arc::clone(&frames);
        let s2c = scope.spawn(move || {
            let mut reader = upstream_reader;
            let result = websocket_copy_frames_shared(
                &mut reader,
                client,
                FrameDirection::ServerToClient,
                trace_limit,
                &s2c_frames,
            );
            if let Some(stream) = client_shutdown {
                let _ = stream.shutdown(Shutdown::Write);
            }
            result
        });
        (
            c2s.join()
                .unwrap_or_else(|_| Err(io::Error::other("websocket c2s thread panicked"))),
            s2c.join()
                .unwrap_or_else(|_| Err(io::Error::other("websocket s2c thread panicked"))),
        )
    });
    let request_bytes = c2s?;
    let response_bytes = s2c?;
    let frames = frames.lock().unwrap().clone();
    Ok((request_bytes, response_bytes, frames))
}

fn websocket_copy_frames_shared<R: Read, W: Write>(
    reader: &mut R,
    writer: &mut W,
    direction: FrameDirection,
    trace_limit: usize,
    frames: &Arc<Mutex<Vec<FrameRecord>>>,
) -> io::Result<u64> {
    let mut bytes = 0u64;
    let mut trace_state = WsTraceState::default();
    loop {
        let frame = match read_ws_frame(reader) {
            Ok(Some(frame)) => frame,
            Ok(None) => break,
            Err(err) if websocket_end_error(&err) => break,
            Err(err) => return Err(err),
        };
        bytes += frame.raw.len() as u64;
        if let Err(err) = writer.write_all(&frame.raw) {
            if websocket_end_error(&err) {
                break;
            }
            return Err(err);
        }
        if let Err(err) = writer.flush() {
            if websocket_end_error(&err) {
                break;
            }
            return Err(err);
        }
        record_ws_frame(
            &mut frames.lock().unwrap(),
            direction,
            &frame,
            trace_limit,
            &mut trace_state,
        );
        if frame.opcode == 0x8 {
            break;
        }
    }
    Ok(bytes)
}
