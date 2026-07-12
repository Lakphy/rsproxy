use rsproxy_trace::{
    FrameDataEncoding, FrameDirection, FrameRecord, Session, SessionKind, TlsRecord, TraceStats,
};

mod har;

pub use har::sessions_har;

pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len() + 8);
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
    out
}

pub fn string(input: &str) -> String {
    format!("\"{}\"", escape(input))
}

pub fn session_summary(session: &Session) -> String {
    format!(
        "{{\"id\":{},\"kind\":\"{}\",\"method\":{},\"url\":{},\"status\":{},\"duration_ms\":{},\"pool_wait_ms\":{},\"dns_ms\":{},\"connect_ms\":{},\"ttfb_ms\":{},\"request_send_ms\":{},\"response_receive_ms\":{},\"response_bytes\":{},\"rules\":[{}],\"error\":{}}}",
        session.id,
        kind(session.kind),
        string(&session.method),
        string(&session.url),
        opt_u16(session.status),
        session.duration_ms,
        session.pool_wait_ms,
        session.dns_ms,
        session.connect_ms,
        session.ttfb_ms,
        opt_u64(session.request_send_ms),
        opt_u64(session.response_receive_ms),
        session.response_bytes,
        session
            .matched_rules
            .iter()
            .map(|r| format!(
                "{{\"group\":{},\"line\":{},\"raw\":{}}}",
                string(&r.group),
                r.line,
                string(&rsproxy_rules::redact_secrets(&r.raw))
            ))
            .collect::<Vec<_>>()
            .join(","),
        opt_string(session.error.as_deref())
    )
}

pub fn session_detail(session: &Session) -> String {
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
        string(&session.method),
        string(&session.url),
        opt_u16(session.status),
        string(&session.client),
        opt_string(session.upstream.as_deref()),
        session.request_bytes,
        session.response_bytes,
        session
            .flags
            .iter()
            .map(|f| string(f))
            .collect::<Vec<_>>()
            .join(","),
        session
            .matched_rules
            .iter()
            .map(|r| format!(
                "{{\"group\":{},\"line\":{},\"raw\":{}}}",
                string(&r.group),
                r.line,
                string(&rsproxy_rules::redact_secrets(&r.raw))
            ))
            .collect::<Vec<_>>()
            .join(","),
        headers(&session.req_headers),
        headers(&session.req_trailers),
        string(&String::from_utf8_lossy(&session.req_body_head)),
        headers(&session.res_headers),
        headers(&session.res_trailers),
        string(&String::from_utf8_lossy(&session.res_body_head)),
        frames(session),
        tls_records(&session.tls),
        opt_string(session.error.as_deref())
    )
}

pub fn sessions_json(sessions: &[Session]) -> String {
    format!(
        "[{}]",
        sessions
            .iter()
            .map(session_detail)
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub fn sessions_table(sessions: &[Session]) -> String {
    let mut out = String::from("ID    KIND    STATUS  DUR_MS  BYTES   METHOD  URL\n");
    for session in sessions {
        out.push_str(&format!(
            "{:<5} {:<7} {:<6} {:<7} {:<7} {:<7} {}\n",
            session.id,
            kind(session.kind),
            session
                .status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".to_string()),
            session.duration_ms,
            session.response_bytes,
            session.method,
            truncate(&session.url, 90)
        ));
    }
    out
}

pub fn stats(stats: TraceStats) -> String {
    format!(
        "{{\"sessions\":{},\"max_sessions\":{},\"dropped\":{},\"queue_dropped\":{},\"queue_capacity\":{},\"queue_bytes\":{},\"queue_memory_budget_bytes\":{},\"queue_memory_dropped\":{},\"evicted_sessions\":{},\"memory_bytes\":{},\"completed_memory_bytes\":{},\"pending_memory_bytes\":{},\"resident_memory_budget_bytes\":{},\"total_memory_bytes\":{},\"memory_budget_bytes\":{},\"next_id\":{},\"pending_sessions\":{},\"incomplete_sessions\":{},\"orphan_events\":{},\"follow_subscribers\":{},\"follow_dropped\":{},\"spilled\":{},\"spill_path\":{},\"spill_dir\":{},\"spill_bytes\":{},\"spill_segments\":{},\"spill_segment_bytes\":{},\"spill_disk_budget_bytes\":{},\"spill_compression\":{},\"spill_evicted_segments\":{},\"spill_errors\":{},\"last_spill_error\":{},\"spill_index_entries\":{},\"spill_corrupt_records\":{}}}",
        stats.sessions,
        stats.max_sessions,
        stats.dropped,
        stats.queue_dropped,
        stats.queue_capacity,
        stats.queue_bytes,
        stats.queue_memory_budget_bytes,
        stats.queue_memory_dropped,
        stats.evicted_sessions,
        stats.memory_bytes,
        stats.completed_memory_bytes,
        stats.pending_memory_bytes,
        stats.resident_memory_budget_bytes,
        stats.total_memory_bytes,
        stats.memory_budget_bytes,
        stats.next_id,
        stats.pending_sessions,
        stats.incomplete_sessions,
        stats.orphan_events,
        stats.follow_subscribers,
        stats.follow_dropped,
        stats.spilled,
        opt_string(stats.spill_path.as_deref()),
        opt_string(stats.spill_dir.as_deref()),
        stats.spill_bytes,
        stats.spill_segments,
        stats.spill_segment_bytes,
        stats.spill_disk_budget_bytes,
        opt_string(stats.spill_compression.as_deref()),
        stats.spill_evicted_segments,
        stats.spill_errors,
        opt_string(stats.last_spill_error.as_deref()),
        stats.spill_index_entries,
        stats.spill_corrupt_records
    )
}

pub fn headers(headers: &[(String, String)]) -> String {
    format!(
        "[{}]",
        headers
            .iter()
            .map(|(k, v)| format!("[{},{}]", string(k), string(v)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

pub fn opt_string(value: Option<&str>) -> String {
    value.map(string).unwrap_or_else(|| "null".to_string())
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
                string(&frame.opcode),
                frame.fin,
                frame.payload_len,
                frame.preview_len(),
                frame.data_encoding.name(),
                frame_data(frame),
                frame.truncated
            ))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn tls_records(records: &[TlsRecord]) -> String {
    format!(
        "[{}]",
        records
            .iter()
            .map(|record| format!(
                "{{\"phase\":{},\"host\":{},\"handshake_ms\":{},\"peer_certificates\":{},\"protocol\":{},\"cipher_suite\":{},\"alpn\":{},\"error\":{}}}",
                string(&record.phase),
                string(&record.host),
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

fn frame_data(frame: &FrameRecord) -> String {
    match frame.data_encoding {
        FrameDataEncoding::Utf8 => string(&String::from_utf8_lossy(&frame.data)),
        FrameDataEncoding::Hex => string(&hex_lower(&frame.data)),
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

fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    let keep = max.saturating_sub(3);
    let mut out = input.chars().take(keep).collect::<String>();
    out.push_str("...");
    out
}

#[cfg(test)]
#[path = "json/tests/mod.rs"]
mod tests;
