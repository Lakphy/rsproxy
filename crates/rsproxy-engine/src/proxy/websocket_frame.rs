use super::*;

pub(super) fn record_ws_frame(
    frames: &mut Vec<FrameRecord>,
    direction: FrameDirection,
    frame: &WsFrame,
    trace_limit: usize,
    state: &mut WsTraceState,
) {
    if frames.len() >= 512 {
        update_ws_trace_state(state, frame);
        return;
    }
    let logical_opcode = if frame.opcode == 0x0 {
        state.fragmented_opcode.unwrap_or(0x0)
    } else {
        frame.opcode
    };
    let data_encoding = ws_frame_encoding(logical_opcode, &frame.payload);
    frames.push(FrameRecord {
        direction,
        at_ms: rsproxy_trace::now_millis(),
        opcode: ws_opcode_name(frame.opcode).to_string(),
        fin: frame.fin,
        payload_len: frame.payload.len() as u64,
        data_encoding,
        data: frame.payload.iter().copied().take(trace_limit).collect(),
        truncated: frame.payload.len() > trace_limit,
    });
    update_ws_trace_state(state, frame);
}

pub(super) fn update_ws_trace_state(state: &mut WsTraceState, frame: &WsFrame) {
    match frame.opcode {
        0x1 | 0x2 if !frame.fin => state.fragmented_opcode = Some(frame.opcode),
        0x1 | 0x2 => state.fragmented_opcode = None,
        0x0 if frame.fin => state.fragmented_opcode = None,
        _ => {}
    }
}

pub(super) fn ws_frame_encoding(logical_opcode: u8, payload: &[u8]) -> FrameDataEncoding {
    match logical_opcode {
        0x1 => FrameDataEncoding::Utf8,
        0x8..=0xA if std::str::from_utf8(payload).is_ok() => FrameDataEncoding::Utf8,
        _ => FrameDataEncoding::Hex,
    }
}

pub(super) fn ws_opcode_name(opcode: u8) -> &'static str {
    match opcode {
        0x0 => "continuation",
        0x1 => "text",
        0x2 => "binary",
        0x8 => "close",
        0x9 => "ping",
        0xA => "pong",
        _ => "unknown",
    }
}

pub(super) fn read_ws_frame<R: Read + ?Sized>(stream: &mut R) -> io::Result<Option<WsFrame>> {
    let mut first = [0u8; 2];
    match stream.read_exact(&mut first) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(err) => return Err(err),
    }

    let fin = first[0] & 0x80 != 0;
    let opcode = first[0] & 0x0f;
    let masked = first[1] & 0x80 != 0;
    let mut len = (first[1] & 0x7f) as u64;
    let mut raw = first.to_vec();

    if len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        raw.extend_from_slice(&ext);
        len = u16::from_be_bytes(ext) as u64;
    } else if len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        raw.extend_from_slice(&ext);
        len = u64::from_be_bytes(ext);
    }
    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "websocket frame too large",
        ));
    }

    let mask = if masked {
        let mut key = [0u8; 4];
        stream.read_exact(&mut key)?;
        raw.extend_from_slice(&key);
        Some(key)
    } else {
        None
    };

    let mut payload = vec![0u8; len as usize];
    stream.read_exact(&mut payload)?;
    raw.extend_from_slice(&payload);
    if let Some(mask) = mask {
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
    }

    Ok(Some(WsFrame {
        raw,
        payload,
        opcode,
        fin,
    }))
}

pub(super) fn parse_ws_frame_prefix(buf: &[u8]) -> io::Result<Option<(WsFrame, usize)>> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let fin = buf[0] & 0x80 != 0;
    let opcode = buf[0] & 0x0f;
    let masked = buf[1] & 0x80 != 0;
    let mut len = (buf[1] & 0x7f) as u64;
    let mut pos = 2usize;

    if len == 126 {
        if buf.len() < pos + 2 {
            return Ok(None);
        }
        len = u16::from_be_bytes([buf[pos], buf[pos + 1]]) as u64;
        pos += 2;
    } else if len == 127 {
        if buf.len() < pos + 8 {
            return Ok(None);
        }
        len = u64::from_be_bytes([
            buf[pos],
            buf[pos + 1],
            buf[pos + 2],
            buf[pos + 3],
            buf[pos + 4],
            buf[pos + 5],
            buf[pos + 6],
            buf[pos + 7],
        ]);
        pos += 8;
    }
    if len > 16 * 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "websocket frame too large",
        ));
    }

    let mask = if masked {
        if buf.len() < pos + 4 {
            return Ok(None);
        }
        let key = [buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]];
        pos += 4;
        Some(key)
    } else {
        None
    };

    let end = pos + len as usize;
    if buf.len() < end {
        return Ok(None);
    }
    let raw = buf[..end].to_vec();
    let mut payload = buf[pos..end].to_vec();
    if let Some(mask) = mask {
        for (idx, byte) in payload.iter_mut().enumerate() {
            *byte ^= mask[idx % 4];
        }
    }
    Ok(Some((
        WsFrame {
            raw,
            payload,
            opcode,
            fin,
        },
        end,
    )))
}
