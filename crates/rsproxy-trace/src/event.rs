use crate::model::{FrameRecord, Session, SessionKind, TlsRecord};
use bytes::Bytes;
use rsproxy_rules::MatchedRule;
use std::mem::size_of;

#[derive(Clone, Debug)]
pub struct SessionStart {
    pub kind: SessionKind,
    pub started_ms: u64,
    pub method: String,
    pub url: String,
    pub client: String,
}

impl SessionStart {
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
pub enum BodyDirection {
    Request,
    Response,
}

#[derive(Clone, Debug)]
pub enum TraceEvent {
    Start {
        id: u64,
        start: SessionStart,
    },
    Request {
        id: u64,
        method: Option<String>,
        url: Option<String>,
        headers: Vec<(String, String)>,
        trailers: Vec<(String, String)>,
        matched_rules: Vec<MatchedRule>,
    },
    Response {
        id: u64,
        status: Option<u16>,
        headers: Vec<(String, String)>,
        trailers: Vec<(String, String)>,
    },
    BodyChunk {
        id: u64,
        direction: BodyDirection,
        data: Bytes,
        observed_bytes: u64,
    },
    BodySnapshot {
        id: u64,
        direction: BodyDirection,
        data: Bytes,
        observed_bytes: u64,
    },
    Frame {
        id: u64,
        frame: FrameRecord,
    },
    Tls {
        id: u64,
        record: TlsRecord,
    },
    End {
        id: u64,
        kind: SessionKind,
        duration_ms: u64,
        pool_wait_ms: u64,
        dns_ms: u64,
        connect_ms: u64,
        ttfb_ms: u64,
        request_send_ms: Option<u64>,
        response_receive_ms: Option<u64>,
        upstream: Option<String>,
        flags: Vec<String>,
        error: Option<String>,
    },
    Abort {
        id: u64,
    },
}

impl TraceEvent {
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
            .map(|rule| rule.group.capacity().saturating_add(rule.raw.capacity()))
            .sum(),
    )
}

fn option_string_bytes(value: &Option<String>) -> usize {
    value.as_ref().map(String::capacity).unwrap_or(0)
}

fn vector_storage_bytes<T>(capacity: usize) -> usize {
    capacity.saturating_mul(size_of::<T>())
}
