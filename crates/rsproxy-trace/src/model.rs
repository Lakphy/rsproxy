use rsproxy_rules::MatchedRule;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
pub struct Session {
    pub id: u64,
    pub kind: SessionKind,
    pub started_ms: u64,
    pub duration_ms: u64,
    pub pool_wait_ms: u64,
    pub dns_ms: u64,
    pub connect_ms: u64,
    pub ttfb_ms: u64,
    pub request_send_ms: Option<u64>,
    pub response_receive_ms: Option<u64>,
    pub method: String,
    pub url: String,
    pub status: Option<u16>,
    pub client: String,
    pub upstream: Option<String>,
    pub request_bytes: u64,
    pub response_bytes: u64,
    pub matched_rules: Vec<MatchedRule>,
    pub flags: Vec<String>,
    pub error: Option<String>,
    pub req_headers: Vec<(String, String)>,
    pub req_trailers: Vec<(String, String)>,
    pub req_body_head: Vec<u8>,
    pub res_headers: Vec<(String, String)>,
    pub res_trailers: Vec<(String, String)>,
    pub res_body_head: Vec<u8>,
    pub frames: Vec<FrameRecord>,
    pub tls: Vec<TlsRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionKind {
    Http,
    Tunnel,
    Sse,
    WebSocket,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TlsRecord {
    pub phase: String,
    pub host: String,
    pub handshake_ms: u64,
    pub peer_certificates: usize,
    pub protocol: Option<String>,
    pub cipher_suite: Option<String>,
    pub alpn: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct FrameRecord {
    pub direction: FrameDirection,
    pub at_ms: u64,
    pub opcode: String,
    pub fin: bool,
    pub payload_len: u64,
    pub data_encoding: FrameDataEncoding,
    pub data: Vec<u8>,
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameDirection {
    ClientToServer,
    ServerToClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameDataEncoding {
    Utf8,
    Hex,
}

impl FrameDataEncoding {
    pub fn name(self) -> &'static str {
        match self {
            FrameDataEncoding::Utf8 => "utf8",
            FrameDataEncoding::Hex => "hex",
        }
    }
}

impl FrameRecord {
    pub fn new(
        direction: FrameDirection,
        at_ms: u64,
        opcode: impl Into<String>,
        fin: bool,
        payload: &[u8],
        preview_limit: usize,
        data_encoding: FrameDataEncoding,
    ) -> Self {
        Self {
            direction,
            at_ms,
            opcode: opcode.into(),
            fin,
            payload_len: payload.len() as u64,
            data_encoding,
            data: payload.iter().copied().take(preview_limit).collect(),
            truncated: payload.len() > preview_limit,
        }
    }

    pub fn preview_len(&self) -> usize {
        self.data.len()
    }
}

impl Session {
    pub fn new(kind: SessionKind, method: String, url: String, client: String) -> Self {
        Self {
            id: 0,
            kind,
            started_ms: now_millis(),
            duration_ms: 0,
            pool_wait_ms: 0,
            dns_ms: 0,
            connect_ms: 0,
            ttfb_ms: 0,
            request_send_ms: None,
            response_receive_ms: None,
            method,
            url,
            status: None,
            client,
            upstream: None,
            request_bytes: 0,
            response_bytes: 0,
            matched_rules: Vec::new(),
            flags: Vec::new(),
            error: None,
            req_headers: Vec::new(),
            req_trailers: Vec::new(),
            req_body_head: Vec::new(),
            res_headers: Vec::new(),
            res_trailers: Vec::new(),
            res_body_head: Vec::new(),
            frames: Vec::new(),
            tls: Vec::new(),
        }
    }

    pub fn finish(&mut self) {
        self.duration_ms = now_millis().saturating_sub(self.started_ms);
    }
}

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
