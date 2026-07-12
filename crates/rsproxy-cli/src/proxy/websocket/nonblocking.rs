use super::*;

pub(super) fn websocket_tunnel_nonblocking<W: WsIo>(
    client: &mut W,
    upstream: &mut UpstreamStream,
    trace_limit: usize,
) -> io::Result<(u64, u64, Vec<FrameRecord>)> {
    client.set_ws_nonblocking(true)?;
    upstream.set_nonblocking(true)?;
    let result = websocket_tunnel_nonblocking_inner(client, upstream, trace_limit);
    let client_blocking = client.set_ws_nonblocking(false);
    let upstream_blocking = upstream.set_nonblocking(false);
    match (result, client_blocking, upstream_blocking) {
        (Err(err), _, _) => Err(err),
        (Ok(_), Err(err), _) | (Ok(_), _, Err(err)) => Err(err),
        (Ok(result), Ok(()), Ok(())) => Ok(result),
    }
}

fn websocket_tunnel_nonblocking_inner<W: WsIo>(
    client: &mut W,
    upstream: &mut UpstreamStream,
    trace_limit: usize,
) -> io::Result<(u64, u64, Vec<FrameRecord>)> {
    let mut request_bytes = 0u64;
    let mut response_bytes = 0u64;
    let mut frames = Vec::new();
    let mut c2s_state = WsTraceState::default();
    let mut s2c_state = WsTraceState::default();
    let mut c2s = WsFrameDecoder::default();
    let mut s2c = WsFrameDecoder::default();
    let mut pending_to_upstream = Vec::new();
    let mut pending_to_client = Vec::new();
    let mut client_closed = false;
    let mut upstream_closed = false;
    let mut client_close_seen = false;
    let mut upstream_close_seen = false;

    loop {
        let mut progressed = false;

        if !client_closed && pending_to_upstream.len() < 1024 * 1024 {
            match read_ws_frames_nonblocking(client, &mut c2s) {
                Ok((0, parsed, closed)) => {
                    client_closed |= closed;
                    append_frames(
                        parsed,
                        FrameTarget {
                            bytes: &mut request_bytes,
                            pending: &mut pending_to_upstream,
                            frames: &mut frames,
                            direction: FrameDirection::ClientToServer,
                            trace_limit,
                            trace_state: &mut c2s_state,
                            close_seen: &mut client_close_seen,
                        },
                    );
                }
                Ok((_, parsed, closed)) => {
                    progressed = true;
                    client_closed |= closed;
                    append_frames(
                        parsed,
                        FrameTarget {
                            bytes: &mut request_bytes,
                            pending: &mut pending_to_upstream,
                            frames: &mut frames,
                            direction: FrameDirection::ClientToServer,
                            trace_limit,
                            trace_state: &mut c2s_state,
                            close_seen: &mut client_close_seen,
                        },
                    );
                }
                Err(err) if websocket_end_error(&err) => client_closed = true,
                Err(err) => return Err(err),
            }
        }

        if !upstream_closed && pending_to_client.len() < 1024 * 1024 {
            match read_ws_frames_nonblocking(upstream, &mut s2c) {
                Ok((0, parsed, closed)) => {
                    upstream_closed |= closed;
                    append_frames(
                        parsed,
                        FrameTarget {
                            bytes: &mut response_bytes,
                            pending: &mut pending_to_client,
                            frames: &mut frames,
                            direction: FrameDirection::ServerToClient,
                            trace_limit,
                            trace_state: &mut s2c_state,
                            close_seen: &mut upstream_close_seen,
                        },
                    );
                }
                Ok((_, parsed, closed)) => {
                    progressed = true;
                    upstream_closed |= closed;
                    append_frames(
                        parsed,
                        FrameTarget {
                            bytes: &mut response_bytes,
                            pending: &mut pending_to_client,
                            frames: &mut frames,
                            direction: FrameDirection::ServerToClient,
                            trace_limit,
                            trace_state: &mut s2c_state,
                            close_seen: &mut upstream_close_seen,
                        },
                    );
                }
                Err(err) if websocket_end_error(&err) => upstream_closed = true,
                Err(err) => return Err(err),
            }
        }

        let to_upstream = flush_pending_nonblocking(upstream, &mut pending_to_upstream)?;
        let to_client = flush_pending_nonblocking(client, &mut pending_to_client)?;
        progressed |= to_upstream > 0 || to_client > 0;

        if client_close_seen && pending_to_upstream.is_empty() {
            let _ = upstream.shutdown(Shutdown::Write);
            client_closed = true;
        }
        if upstream_close_seen && pending_to_client.is_empty() {
            let _ = client.shutdown_ws(Shutdown::Write);
            upstream_closed = true;
        }

        if ((client_closed && upstream_closed) || (client_close_seen && upstream_close_seen))
            && pending_to_upstream.is_empty()
            && pending_to_client.is_empty()
        {
            break;
        }

        if !progressed {
            thread::sleep(Duration::from_millis(1));
        }
    }

    Ok((request_bytes, response_bytes, frames))
}

struct FrameTarget<'a> {
    bytes: &'a mut u64,
    pending: &'a mut Vec<u8>,
    frames: &'a mut Vec<FrameRecord>,
    direction: FrameDirection,
    trace_limit: usize,
    trace_state: &'a mut WsTraceState,
    close_seen: &'a mut bool,
}

fn append_frames(parsed: Vec<WsFrame>, target: FrameTarget<'_>) {
    for frame in parsed {
        *target.bytes += frame.raw.len() as u64;
        target.pending.extend_from_slice(&frame.raw);
        record_ws_frame(
            target.frames,
            target.direction,
            &frame,
            target.trace_limit,
            target.trace_state,
        );
        *target.close_seen |= frame.opcode == 0x8;
    }
}

pub(in crate::proxy) fn read_ws_frames_nonblocking<R: Read>(
    reader: &mut R,
    decoder: &mut WsFrameDecoder,
) -> io::Result<(usize, Vec<WsFrame>, bool)> {
    let mut total = 0usize;
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => return Ok((total, out, true)),
            Ok(n) => {
                total += n;
                out.extend(decoder.push(&buf[..n])?);
                if n < buf.len() {
                    return Ok((total, out, false));
                }
            }
            Err(err) if would_block(&err) => return Ok((total, out, false)),
            Err(err) => return Err(err),
        }
    }
}

pub(in crate::proxy) fn flush_pending_nonblocking<W: Write>(
    writer: &mut W,
    pending: &mut Vec<u8>,
) -> io::Result<usize> {
    let mut written = 0usize;
    while !pending.is_empty() {
        match writer.write(pending) {
            Ok(0) => break,
            Ok(n) => {
                pending.drain(..n);
                written += n;
            }
            Err(err) if would_block(&err) => break,
            Err(err) if websocket_end_error(&err) => break,
            Err(err) => return Err(err),
        }
    }
    if written > 0 {
        match writer.flush() {
            Ok(()) => {}
            Err(err) if would_block(&err) || websocket_end_error(&err) => {}
            Err(err) => return Err(err),
        }
    }
    Ok(written)
}
