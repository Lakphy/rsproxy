use crate::model::{FrameDataEncoding, FrameDirection, FrameRecord, Session, SessionKind};
use rsproxy_rules::redact_secrets;

pub(super) fn spill_session_line(session: &Session) -> String {
    format!(
        "{{\"id\":{},\"kind\":\"{}\",\"started_ms\":{},\"duration_ms\":{},\"pool_wait_ms\":{},\"dns_ms\":{},\"connect_ms\":{},\"ttfb_ms\":{},\"request_send_ms\":{},\"response_receive_ms\":{},\"method\":{},\"url\":{},\"status\":{},\"client\":{},\"upstream\":{},\"request_bytes\":{},\"response_bytes\":{},\"flags\":[{}],\"rules\":[{}],\"req_headers\":{},\"req_trailers\":{},\"req_body_head\":{},\"res_headers\":{},\"res_trailers\":{},\"res_body_head\":{},\"frames\":{},\"tls\":{},\"error\":{}}}",
        session.id,
        kind(session.kind),
        session.started_ms,
        session.duration_ms,
        session.pool_wait_ms,
        session.dns_ms,
        session.connect_ms,
        session.ttfb_ms,
        opt_u64(session.request_send_ms),
        opt_u64(session.response_receive_ms),
        json_string(&session.method),
        json_string(&session.url),
        opt_u16(session.status),
        json_string(&session.client),
        opt_string(session.upstream.as_deref()),
        session.request_bytes,
        session.response_bytes,
        session
            .flags
            .iter()
            .map(|flag| json_string(flag))
            .collect::<Vec<_>>()
            .join(","),
        session
            .matched_rules
            .iter()
            .map(|rule| format!(
                "{{\"group\":{},\"line\":{},\"raw\":{}}}",
                json_string(&rule.group),
                rule.line,
                json_string(&redact_secrets(&rule.raw))
            ))
            .collect::<Vec<_>>()
            .join(","),
        headers(&session.req_headers),
        headers(&session.req_trailers),
        json_string(&String::from_utf8_lossy(&session.req_body_head)),
        headers(&session.res_headers),
        headers(&session.res_trailers),
        json_string(&String::from_utf8_lossy(&session.res_body_head)),
        frames(session),
        tls_records(session),
        opt_string(session.error.as_deref())
    )
}

fn headers(headers: &[(String, String)]) -> String {
    format!(
        "[{}]",
        headers
            .iter()
            .map(|(name, value)| format!("[{},{}]", json_string(name), json_string(value)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn frames(session: &Session) -> String {
    format!(
        "[{}]",
        session
            .frames
            .iter()
            .map(|frame| format!(
                "{{\"direction\":\"{}\",\"at_ms\":{},\"opcode\":{},\"fin\":{},\"payload_len\":{},\"preview_len\":{},\"data_encoding\":\"{}\",\"data\":{},\"truncated\":{}}}",
                match frame.direction {
                    FrameDirection::ClientToServer => "c2s",
                    FrameDirection::ServerToClient => "s2c",
                },
                frame.at_ms,
                json_string(&frame.opcode),
                frame.fin,
                frame.payload_len,
                frame.preview_len(),
                frame.data_encoding.name(),
                frame_data_json(frame),
                frame.truncated
            ))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn tls_records(session: &Session) -> String {
    format!(
        "[{}]",
        session
            .tls
            .iter()
            .map(|record| format!(
                "{{\"phase\":{},\"host\":{},\"handshake_ms\":{},\"peer_certificates\":{},\"protocol\":{},\"cipher_suite\":{},\"alpn\":{},\"error\":{}}}",
                json_string(&record.phase),
                json_string(&record.host),
                record.handshake_ms,
                record.peer_certificates,
                opt_string(record.protocol.as_deref()),
                opt_string(record.cipher_suite.as_deref()),
                opt_string(record.alpn.as_deref()),
                opt_string(record.error.as_deref())
            ))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn frame_data_json(frame: &FrameRecord) -> String {
    match frame.data_encoding {
        FrameDataEncoding::Utf8 => json_string(&String::from_utf8_lossy(&frame.data)),
        FrameDataEncoding::Hex => json_string(&hex_lower(&frame.data)),
    }
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}

fn opt_string(value: Option<&str>) -> String {
    value.map(json_string).unwrap_or_else(|| "null".to_string())
}

fn opt_u16(value: Option<u16>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn opt_u64(value: Option<u64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string())
}

fn kind(kind: SessionKind) -> &'static str {
    match kind {
        SessionKind::Http => "http",
        SessionKind::Tunnel => "tunnel",
        SessionKind::Sse => "sse",
        SessionKind::WebSocket => "websocket",
    }
}

fn json_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 2);
    out.push('"');
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
