use rsproxy_rules::MatchedRule;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug)]
/// A completed trace session exposed to readers and serialized at the control boundary.
pub struct Session {
    /// Store-local monotonically increasing identifier; zero means not yet assigned.
    pub id: u64,
    /// Protocol lifecycle represented by this record.
    pub kind: SessionKind,
    /// Unix timestamp in milliseconds captured when processing began.
    pub started_ms: u64,
    /// End-to-end elapsed time in milliseconds.
    pub duration_ms: u64,
    /// Time spent waiting for upstream pool capacity, in milliseconds.
    pub pool_wait_ms: u64,
    /// Time spent resolving the upstream address, in milliseconds.
    pub dns_ms: u64,
    /// Time spent establishing the upstream transport, in milliseconds.
    pub connect_ms: u64,
    /// Time from completed request send to first response byte, in milliseconds.
    pub ttfb_ms: u64,
    /// Time spent sending the request, in milliseconds, when measured.
    pub request_send_ms: Option<u64>,
    /// Time spent receiving the response, in milliseconds, when measured.
    pub response_receive_ms: Option<u64>,
    /// Request method or protocol-specific operation label.
    pub method: String,
    /// Request URL or tunnel destination as observed by the engine.
    pub url: String,
    /// HTTP response status, absent when no response head was received.
    pub status: Option<u16>,
    /// Display form of the downstream peer address.
    pub client: String,
    /// Selected upstream endpoint or route description.
    pub upstream: Option<String>,
    /// Total request-body bytes observed, including bytes omitted from the preview.
    pub request_bytes: u64,
    /// Total response-body bytes observed, including bytes omitted from the preview.
    pub response_bytes: u64,
    /// Rules whose actions contributed to processing this session.
    pub matched_rules: Vec<MatchedRule>,
    /// Stable diagnostic labels accumulated by the engine.
    pub flags: Vec<String>,
    /// Sanitized terminal failure message, if processing failed.
    pub error: Option<String>,
    /// Request headers captured as wire-independent name/value pairs.
    pub req_headers: Vec<(String, String)>,
    /// Request trailers captured after the request body.
    pub req_trailers: Vec<(String, String)>,
    /// Prefix of the request body, bounded by the store's body preview limit.
    pub req_body_head: Vec<u8>,
    /// Response headers captured as wire-independent name/value pairs.
    pub res_headers: Vec<(String, String)>,
    /// Response trailers captured after the response body.
    pub res_trailers: Vec<(String, String)>,
    /// Prefix of the response body, bounded by the store's body preview limit.
    pub res_body_head: Vec<u8>,
    /// Bounded frame records observed for framed protocols.
    pub frames: Vec<FrameRecord>,
    /// TLS handshakes performed while serving the session.
    pub tls: Vec<TlsRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// High-level transport lifecycle used to interpret a trace session.
pub enum SessionKind {
    /// A finite HTTP request and response exchange.
    Http,
    /// An opaque byte tunnel such as a non-intercepted CONNECT connection.
    Tunnel,
    /// A long-lived server-sent events response.
    Sse,
    /// A WebSocket session with frame-level observations.
    WebSocket,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Sanitized metadata for one TLS handshake associated with a session.
pub struct TlsRecord {
    /// Engine-defined handshake phase, such as downstream or upstream.
    pub phase: String,
    /// Server name or endpoint used for the handshake.
    pub host: String,
    /// Elapsed handshake time in milliseconds.
    pub handshake_ms: u64,
    /// Number of peer certificates presented during validation.
    pub peer_certificates: usize,
    /// Negotiated TLS protocol version, when available.
    pub protocol: Option<String>,
    /// Negotiated cipher suite, when available.
    pub cipher_suite: Option<String>,
    /// Negotiated ALPN protocol, when available.
    pub alpn: Option<String>,
    /// Sanitized handshake failure message, if negotiation failed.
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
/// Metadata and a bounded payload preview for one framed-protocol message.
pub struct FrameRecord {
    /// Direction in which the frame crossed the proxy.
    pub direction: FrameDirection,
    /// Milliseconds elapsed from session start when the frame was observed.
    pub at_ms: u64,
    /// Protocol opcode rendered as a stable textual label.
    pub opcode: String,
    /// Whether this frame carries the final fragment of its message.
    pub fin: bool,
    /// Full payload length in bytes before preview truncation.
    pub payload_len: u64,
    /// Interpretation used when rendering the preview bytes.
    pub data_encoding: FrameDataEncoding,
    /// Payload prefix bounded by the limit supplied to [`FrameRecord::new`].
    pub data: Vec<u8>,
    /// Whether the full payload is longer than [`FrameRecord::data`].
    pub truncated: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Direction in which a frame crossed the proxy.
pub enum FrameDirection {
    /// From the downstream client toward the upstream server.
    ClientToServer,
    /// From the upstream server toward the downstream client.
    ServerToClient,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Stable rendering mode for captured frame preview bytes.
pub enum FrameDataEncoding {
    /// Preview bytes are valid UTF-8 and may be rendered as text.
    Utf8,
    /// Preview bytes must be rendered as hexadecimal data.
    Hex,
}

impl FrameDataEncoding {
    /// Returns the lowercase token used by serialized trace contracts.
    pub fn name(self) -> &'static str {
        match self {
            FrameDataEncoding::Utf8 => "utf8",
            FrameDataEncoding::Hex => "hex",
        }
    }
}

impl FrameRecord {
    /// Captures frame metadata and at most `preview_limit` payload bytes.
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

    /// Returns the number of retained preview bytes, not the full payload length.
    pub fn preview_len(&self) -> usize {
        self.data.len()
    }
}

impl Session {
    /// Starts an empty session using the current Unix time and no store identifier.
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

    /// Recomputes end-to-end duration from [`Session::started_ms`] to the current clock reading.
    pub fn finish(&mut self) {
        self.duration_ms = now_millis().saturating_sub(self.started_ms);
    }
}

/// Reads the system clock once and returns milliseconds since the Unix epoch.
///
/// Clocks before the epoch are represented as zero.
pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
