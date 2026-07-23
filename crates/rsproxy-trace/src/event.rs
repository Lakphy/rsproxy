use crate::model::{FrameRecord, Session, SessionKind, TlsRecord};
use bytes::Bytes;
use rsproxy_rules::MatchedRule;
use std::mem::size_of;

#[derive(Clone, Debug)]
/// Immutable metadata required to open a trace session before later events arrive.
pub struct SessionStart {
    /// Protocol lifecycle represented by the session.
    pub kind: SessionKind,
    /// Unix timestamp in milliseconds captured when processing began.
    pub started_ms: u64,
    /// Initial request method, or a protocol-specific label for non-HTTP sessions.
    pub method: String,
    /// Initial request URL or tunnel destination.
    pub url: String,
    /// Display form of the downstream peer address.
    pub client: String,
}

impl SessionStart {
    /// Copies the opening metadata from a completed or partially built session.
    pub fn from_session(session: &Session) -> Self {
        Self {
            kind: session.kind,
            started_ms: session.started_ms,
            method: session.method.clone(),
            url: session.url.clone(),
            client: session.client.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Identifies which HTTP message owns a body observation.
pub enum BodyDirection {
    /// Bytes flowing from the downstream client toward the upstream.
    Request,
    /// Bytes flowing from the upstream toward the downstream client.
    Response,
}

#[derive(Clone, Debug)]
/// An incremental update consumed by [`crate::TraceStore`]'s collector.
///
/// Events may be submitted concurrently. Except for [`TraceEvent::Start`], an event whose `id`
/// has no pending session is counted as orphaned rather than creating an implicit session.
pub enum TraceEvent {
    /// Opens a pending session under a store-assigned identifier.
    Start {
        /// Store-local monotonically increasing session identifier.
        id: u64,
        /// Metadata captured at the start of the operation.
        start: SessionStart,
    },
    /// Adds or replaces request metadata for a pending session.
    Request {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
        /// Replacement method; `None` preserves the method supplied at start.
        method: Option<String>,
        /// Replacement URL; `None` preserves the URL supplied at start.
        url: Option<String>,
        /// Complete request header snapshot in wire-independent name/value form.
        headers: Vec<(String, String)>,
        /// Complete request trailer snapshot.
        trailers: Vec<(String, String)>,
        /// Rules whose actions contributed to the request.
        matched_rules: Vec<MatchedRule>,
    },
    /// Adds response metadata for a pending session.
    Response {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
        /// HTTP status when available; absent for failures before a response head.
        status: Option<u16>,
        /// Complete response header snapshot.
        headers: Vec<(String, String)>,
        /// Complete response trailer snapshot.
        trailers: Vec<(String, String)>,
    },
    /// Appends a streamed body observation while respecting the configured preview limit.
    BodyChunk {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
        /// Request or response side receiving the bytes.
        direction: BodyDirection,
        /// Newly observed bytes, which may exceed the remaining preview capacity.
        data: Bytes,
        /// New body bytes represented by this event, including bytes omitted from the preview.
        observed_bytes: u64,
    },
    /// Replaces a body preview with an already bounded snapshot.
    BodySnapshot {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
        /// Request or response side represented by the snapshot.
        direction: BodyDirection,
        /// Caller-provided preview bytes.
        data: Bytes,
        /// Total body length represented by the snapshot, including omitted bytes.
        observed_bytes: u64,
    },
    /// Appends a WebSocket or other framed-protocol record.
    Frame {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
        /// Bounded frame metadata and payload preview.
        frame: FrameRecord,
    },
    /// Appends a TLS handshake observation.
    Tls {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
        /// Handshake metadata safe for trace serialization.
        record: TlsRecord,
    },
    /// Completes a pending session and makes it visible to readers and followers.
    End {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
        /// Final protocol classification.
        kind: SessionKind,
        /// End-to-end elapsed time in milliseconds.
        duration_ms: u64,
        /// Time spent waiting for upstream pool capacity, in milliseconds.
        pool_wait_ms: u64,
        /// Time spent resolving the upstream address, in milliseconds.
        dns_ms: u64,
        /// Time spent establishing the upstream transport, in milliseconds.
        connect_ms: u64,
        /// Time from completed request send to first response byte, in milliseconds.
        ttfb_ms: u64,
        /// Time spent sending the request, in milliseconds, when measured.
        request_send_ms: Option<u64>,
        /// Time spent receiving the response, in milliseconds, when measured.
        response_receive_ms: Option<u64>,
        /// Selected upstream endpoint or route description.
        upstream: Option<String>,
        /// Stable diagnostic labels accumulated by the engine.
        flags: Vec<String>,
        /// Sanitized terminal failure message, if the session failed.
        error: Option<String>,
    },
    /// Discards pending state without publishing a completed session.
    Abort {
        /// Identifier returned by [`crate::TraceStore::start`].
        id: u64,
    },
}

impl TraceEvent {
    /// Returns the store-local session identifier carried by every event variant.
    pub fn session_id(&self) -> u64 {
        match self {
            Self::Start { id, .. }
            | Self::Request { id, .. }
            | Self::Response { id, .. }
            | Self::BodyChunk { id, .. }
            | Self::BodySnapshot { id, .. }
            | Self::Frame { id, .. }
            | Self::Tls { id, .. }
            | Self::End { id, .. }
            | Self::Abort { id } => *id,
        }
    }

    pub(crate) fn from_session(session: Session) -> Vec<Self> {
        let Session {
            id,
            kind,
            started_ms,
            duration_ms,
            pool_wait_ms,
            dns_ms,
            connect_ms,
            ttfb_ms,
            request_send_ms,
            response_receive_ms,
            method,
            url,
            status,
            client,
            upstream,
            request_bytes,
            response_bytes,
            matched_rules,
            flags,
            error,
            req_headers,
            req_trailers,
            req_body_head,
            res_headers,
            res_trailers,
            res_body_head,
            frames,
            tls,
        } = session;
        let mut events = Vec::with_capacity(7 + frames.len() + tls.len());
        events.push(Self::Start {
            id,
            start: SessionStart {
                kind,
                started_ms,
                method,
                url,
                client,
            },
        });
        events.push(Self::Request {
            id,
            method: None,
            url: None,
            headers: req_headers,
            trailers: req_trailers,
            matched_rules,
        });
        if request_bytes > 0 || !req_body_head.is_empty() {
            events.push(Self::BodySnapshot {
                id,
                direction: BodyDirection::Request,
                data: Bytes::from(req_body_head),
                observed_bytes: request_bytes,
            });
        }
        events.push(Self::Response {
            id,
            status,
            headers: res_headers,
            trailers: res_trailers,
        });
        if response_bytes > 0 || !res_body_head.is_empty() {
            events.push(Self::BodySnapshot {
                id,
                direction: BodyDirection::Response,
                data: Bytes::from(res_body_head),
                observed_bytes: response_bytes,
            });
        }
        events.extend(frames.into_iter().map(|frame| Self::Frame { id, frame }));
        events.extend(tls.into_iter().map(|record| Self::Tls { id, record }));
        events.push(Self::End {
            id,
            kind,
            duration_ms,
            pool_wait_ms,
            dns_ms,
            connect_ms,
            ttfb_ms,
            request_send_ms,
            response_receive_ms,
            upstream,
            flags,
            error,
        });
        events
    }

    pub(crate) fn continuation_from_session(session: Session) -> Vec<Self> {
        let mut events = Self::from_session(session);
        events.remove(0);
        events
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(self.dynamic_bytes())
    }

    fn dynamic_bytes(&self) -> usize {
        match self {
            Self::Start { start, .. } => start
                .method
                .capacity()
                .saturating_add(start.url.capacity())
                .saturating_add(start.client.capacity()),
            Self::Request {
                method,
                url,
                headers,
                trailers,
                matched_rules,
                ..
            } => option_string_bytes(method)
                .saturating_add(option_string_bytes(url))
                .saturating_add(headers_bytes(headers, headers.capacity()))
                .saturating_add(headers_bytes(trailers, trailers.capacity()))
                .saturating_add(rules_bytes(matched_rules, matched_rules.capacity())),
            Self::Response {
                headers, trailers, ..
            } => headers_bytes(headers, headers.capacity())
                .saturating_add(headers_bytes(trailers, trailers.capacity())),
            Self::BodyChunk {
                data,
                observed_bytes,
                ..
            } => data
                .len()
                .max(usize::try_from(*observed_bytes).unwrap_or(usize::MAX)),
            Self::BodySnapshot { data, .. } => data.len(),
            Self::Frame { frame, .. } => frame
                .opcode
                .capacity()
                .saturating_add(frame.data.capacity()),
            Self::Tls { record, .. } => record
                .phase
                .capacity()
                .saturating_add(record.host.capacity())
                .saturating_add(option_string_bytes(&record.protocol))
                .saturating_add(option_string_bytes(&record.cipher_suite))
                .saturating_add(option_string_bytes(&record.alpn))
                .saturating_add(option_string_bytes(&record.error)),
            Self::End {
                upstream,
                flags,
                error,
                ..
            } => option_string_bytes(upstream)
                .saturating_add(option_string_bytes(error))
                .saturating_add(vector_storage_bytes::<String>(flags.capacity()))
                .saturating_add(flags.iter().map(String::capacity).sum()),
            Self::Abort { .. } => 0,
        }
    }
}

fn headers_bytes(headers: &[(String, String)], capacity: usize) -> usize {
    vector_storage_bytes::<(String, String)>(capacity).saturating_add(
        headers
            .iter()
            .map(|(name, value)| name.capacity().saturating_add(value.capacity()))
            .sum(),
    )
}

fn rules_bytes(rules: &[MatchedRule], capacity: usize) -> usize {
    vector_storage_bytes::<MatchedRule>(capacity).saturating_add(
        rules
            .iter()
            .map(|rule| rule.group.len().saturating_add(rule.raw.len()))
            .sum(),
    )
}

fn option_string_bytes(value: &Option<String>) -> usize {
    value.as_ref().map(String::capacity).unwrap_or(0)
}

fn vector_storage_bytes<T>(capacity: usize) -> usize {
    capacity.saturating_mul(size_of::<T>())
}
