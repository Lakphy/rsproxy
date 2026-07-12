use super::memory::estimate_session_bytes;
use crate::event::{BodyDirection, TraceEvent};
use crate::model::Session;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const PENDING_TTL: Duration = Duration::from_secs(300);
const CLEANUP_INTERVAL: Duration = Duration::from_secs(1);

struct PendingSession {
    session: Session,
    touched: Instant,
}

pub(super) struct PendingSessions {
    sessions: HashMap<u64, PendingSession>,
    body_limit: usize,
    incomplete_sessions: u64,
    orphan_events: u64,
    memory_bytes: usize,
    last_cleanup: Instant,
}

impl PendingSessions {
    pub(super) fn new(body_limit: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            body_limit,
            incomplete_sessions: 0,
            orphan_events: 0,
            memory_bytes: 0,
            last_cleanup: Instant::now(),
        }
    }

    pub(super) fn apply(&mut self, event: TraceEvent) -> Option<Session> {
        self.expire_stale();
        match event {
            TraceEvent::Start { id, start } => {
                let mut session = Session::new(start.kind, start.method, start.url, start.client);
                session.id = id;
                session.started_ms = start.started_ms;
                let session_bytes = estimate_session_bytes(&session);
                if let Some(replaced) = self.sessions.insert(
                    id,
                    PendingSession {
                        session,
                        touched: Instant::now(),
                    },
                ) {
                    self.memory_bytes = self
                        .memory_bytes
                        .saturating_sub(estimate_session_bytes(&replaced.session));
                    self.incomplete_sessions = self.incomplete_sessions.saturating_add(1);
                }
                self.memory_bytes = self.memory_bytes.saturating_add(session_bytes);
                None
            }
            TraceEvent::Request {
                id,
                method,
                url,
                headers,
                trailers,
                matched_rules,
            } => {
                self.update(id, move |session| {
                    if let Some(method) = method {
                        session.method = method;
                    }
                    if let Some(url) = url {
                        session.url = url;
                    }
                    session.req_headers = headers;
                    session.req_trailers = trailers;
                    session.matched_rules = matched_rules;
                })?;
                None
            }
            TraceEvent::Response {
                id,
                status,
                headers,
                trailers,
            } => {
                self.update(id, move |session| {
                    session.status = status;
                    session.res_headers = headers;
                    session.res_trailers = trailers;
                })?;
                None
            }
            TraceEvent::BodyChunk {
                id,
                direction,
                data,
                observed_bytes,
            } => {
                let body_limit = self.body_limit;
                self.update(id, move |session| match direction {
                    BodyDirection::Request => {
                        session.request_bytes =
                            session.request_bytes.saturating_add(observed_bytes);
                        append_preview(&mut session.req_body_head, &data, body_limit);
                    }
                    BodyDirection::Response => {
                        session.response_bytes =
                            session.response_bytes.saturating_add(observed_bytes);
                        append_preview(&mut session.res_body_head, &data, body_limit);
                    }
                })?;
                None
            }
            TraceEvent::BodySnapshot {
                id,
                direction,
                data,
                observed_bytes,
            } => {
                let body_limit = self.body_limit;
                self.update(id, move |session| match direction {
                    BodyDirection::Request => {
                        session.request_bytes = observed_bytes;
                        session.req_body_head.clear();
                        append_preview(&mut session.req_body_head, &data, body_limit);
                    }
                    BodyDirection::Response => {
                        session.response_bytes = observed_bytes;
                        session.res_body_head.clear();
                        append_preview(&mut session.res_body_head, &data, body_limit);
                    }
                })?;
                None
            }
            TraceEvent::Frame { id, frame } => {
                self.update(id, move |session| session.frames.push(frame))?;
                None
            }
            TraceEvent::Tls { id, record } => {
                self.update(id, move |session| session.tls.push(record))?;
                None
            }
            TraceEvent::End {
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
            } => {
                let Some(mut pending) = self.sessions.remove(&id) else {
                    self.orphan_events = self.orphan_events.saturating_add(1);
                    return None;
                };
                self.memory_bytes = self
                    .memory_bytes
                    .saturating_sub(estimate_session_bytes(&pending.session));
                pending.session.kind = kind;
                pending.session.duration_ms = duration_ms;
                pending.session.pool_wait_ms = pool_wait_ms;
                pending.session.dns_ms = dns_ms;
                pending.session.connect_ms = connect_ms;
                pending.session.ttfb_ms = ttfb_ms;
                pending.session.request_send_ms = request_send_ms;
                pending.session.response_receive_ms = response_receive_ms;
                pending.session.upstream = upstream;
                pending.session.flags = flags;
                pending.session.error = error;
                Some(pending.session)
            }
            TraceEvent::Abort { id } => {
                if let Some(pending) = self.sessions.remove(&id) {
                    self.memory_bytes = self
                        .memory_bytes
                        .saturating_sub(estimate_session_bytes(&pending.session));
                    self.incomplete_sessions = self.incomplete_sessions.saturating_add(1);
                }
                None
            }
        }
    }

    pub(super) fn clear(&mut self) {
        self.sessions.clear();
        self.memory_bytes = 0;
    }

    pub(super) fn len(&self) -> usize {
        self.sessions.len()
    }

    pub(super) fn incomplete_sessions(&self) -> u64 {
        self.incomplete_sessions
    }

    pub(super) fn orphan_events(&self) -> u64 {
        self.orphan_events
    }

    pub(super) fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub(super) fn abort_for_budget(&mut self, id: u64) {
        if let Some(pending) = self.sessions.remove(&id) {
            self.memory_bytes = self
                .memory_bytes
                .saturating_sub(estimate_session_bytes(&pending.session));
            self.incomplete_sessions = self.incomplete_sessions.saturating_add(1);
        }
    }

    fn update(&mut self, id: u64, update: impl FnOnce(&mut Session)) -> Option<()> {
        let Some(pending) = self.sessions.get_mut(&id) else {
            self.orphan_events = self.orphan_events.saturating_add(1);
            return None;
        };
        let before = estimate_session_bytes(&pending.session);
        update(&mut pending.session);
        let after = estimate_session_bytes(&pending.session);
        self.memory_bytes = self
            .memory_bytes
            .saturating_sub(before)
            .saturating_add(after);
        pending.touched = Instant::now();
        Some(())
    }

    fn expire_stale(&mut self) {
        if self.last_cleanup.elapsed() < CLEANUP_INTERVAL {
            return;
        }
        self.last_cleanup = Instant::now();
        let before = self.sessions.len();
        let mut removed_bytes = 0usize;
        self.sessions.retain(|_, pending| {
            let keep = pending.touched.elapsed() < PENDING_TTL;
            if !keep {
                removed_bytes =
                    removed_bytes.saturating_add(estimate_session_bytes(&pending.session));
            }
            keep
        });
        self.memory_bytes = self.memory_bytes.saturating_sub(removed_bytes);
        self.incomplete_sessions = self
            .incomplete_sessions
            .saturating_add(before.saturating_sub(self.sessions.len()) as u64);
    }
}

fn append_preview(target: &mut Vec<u8>, data: &[u8], body_limit: usize) {
    let remaining = body_limit.saturating_sub(target.len());
    target.extend_from_slice(&data[..data.len().min(remaining)]);
}
