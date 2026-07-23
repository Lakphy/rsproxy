use crate::model::{FrameRecord, Session, TlsRecord};
use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::Arc;

struct StoredSession {
    session: Arc<Session>,
    bytes: usize,
}

pub(super) struct MemoryStore {
    max_sessions: usize,
    memory_budget_bytes: usize,
    memory_bytes: usize,
    evicted_sessions: u64,
    sessions: VecDeque<StoredSession>,
}

impl MemoryStore {
    pub(super) fn new(max_sessions: usize, memory_budget_bytes: usize) -> Self {
        Self {
            max_sessions,
            memory_budget_bytes,
            memory_bytes: 0,
            evicted_sessions: 0,
            sessions: VecDeque::with_capacity(max_sessions.min(4096)),
        }
    }

    pub(super) fn insert(&mut self, session: Arc<Session>) {
        let bytes = estimate_session_bytes(&session);
        if self.max_sessions == 0
            || self.memory_budget_bytes == 0
            || bytes > self.memory_budget_bytes
        {
            self.evicted_sessions = self.evicted_sessions.saturating_add(1);
            return;
        }
        while !self.sessions.is_empty()
            && (self.sessions.len() >= self.max_sessions
                || self.memory_bytes.saturating_add(bytes) > self.memory_budget_bytes)
        {
            self.evict_oldest();
        }
        self.memory_bytes = self.memory_bytes.saturating_add(bytes);
        self.sessions.push_back(StoredSession { session, bytes });
    }

    pub(super) fn list(&self, limit: usize) -> Vec<Session> {
        self.sessions
            .iter()
            .rev()
            .take(limit.min(self.sessions.len()))
            .map(|stored| stored.session.as_ref().clone())
            .collect()
    }

    pub(super) fn list_after(&self, after: u64, limit: usize) -> Vec<Session> {
        self.sessions
            .iter()
            .filter(|stored| stored.session.id > after)
            .take(limit)
            .map(|stored| stored.session.as_ref().clone())
            .collect()
    }

    pub(super) fn list_after_shared(&self, after: u64, limit: usize) -> Vec<Arc<Session>> {
        self.sessions
            .iter()
            .filter(|stored| stored.session.id > after)
            .take(limit)
            .map(|stored| Arc::clone(&stored.session))
            .collect()
    }

    pub(super) fn get(&self, id: u64) -> Option<Session> {
        self.sessions
            .iter()
            .find(|stored| stored.session.id == id)
            .map(|stored| stored.session.as_ref().clone())
    }

    pub(super) fn clear(&mut self) {
        self.sessions.clear();
        self.memory_bytes = 0;
    }

    pub(super) fn len(&self) -> usize {
        self.sessions.len()
    }

    pub(super) fn max_sessions(&self) -> usize {
        self.max_sessions
    }

    pub(super) fn memory_bytes(&self) -> usize {
        self.memory_bytes
    }

    pub(super) fn memory_budget_bytes(&self) -> usize {
        self.memory_budget_bytes
    }

    pub(super) fn evicted_sessions(&self) -> u64 {
        self.evicted_sessions
    }

    pub(super) fn evict_to_budget(&mut self, budget: usize) {
        while self.memory_bytes > budget && !self.sessions.is_empty() {
            self.evict_oldest();
        }
    }

    fn evict_oldest(&mut self) {
        if let Some(stored) = self.sessions.pop_front() {
            self.memory_bytes = self.memory_bytes.saturating_sub(stored.bytes);
            self.evicted_sessions = self.evicted_sessions.saturating_add(1);
        }
    }
}

pub(super) fn estimate_session_bytes(session: &Session) -> usize {
    size_of::<Session>()
        .saturating_add(session.method.capacity())
        .saturating_add(session.url.capacity())
        .saturating_add(session.client.capacity())
        .saturating_add(option_string_bytes(&session.upstream))
        .saturating_add(option_string_bytes(&session.error))
        .saturating_add(headers_bytes(
            &session.req_headers,
            session.req_headers.capacity(),
        ))
        .saturating_add(headers_bytes(
            &session.req_trailers,
            session.req_trailers.capacity(),
        ))
        .saturating_add(headers_bytes(
            &session.res_headers,
            session.res_headers.capacity(),
        ))
        .saturating_add(headers_bytes(
            &session.res_trailers,
            session.res_trailers.capacity(),
        ))
        .saturating_add(session.req_body_head.capacity())
        .saturating_add(session.res_body_head.capacity())
        .saturating_add(vector_storage_bytes::<String>(session.flags.capacity()))
        .saturating_add(session.flags.iter().map(String::capacity).sum::<usize>())
        .saturating_add(vector_storage_bytes::<rsproxy_rules::MatchedRule>(
            session.matched_rules.capacity(),
        ))
        .saturating_add(
            session
                .matched_rules
                .iter()
                .map(|rule| rule.group.len().saturating_add(rule.raw.len()))
                .sum::<usize>(),
        )
        .saturating_add(vector_storage_bytes::<FrameRecord>(
            session.frames.capacity(),
        ))
        .saturating_add(session.frames.iter().map(frame_bytes).sum::<usize>())
        .saturating_add(vector_storage_bytes::<TlsRecord>(session.tls.capacity()))
        .saturating_add(session.tls.iter().map(tls_bytes).sum::<usize>())
}

fn headers_bytes(headers: &[(String, String)], capacity: usize) -> usize {
    vector_storage_bytes::<(String, String)>(capacity).saturating_add(
        headers
            .iter()
            .map(|(name, value)| name.capacity().saturating_add(value.capacity()))
            .sum(),
    )
}

fn frame_bytes(frame: &FrameRecord) -> usize {
    frame
        .opcode
        .capacity()
        .saturating_add(frame.data.capacity())
}

fn tls_bytes(record: &TlsRecord) -> usize {
    record
        .phase
        .capacity()
        .saturating_add(record.host.capacity())
        .saturating_add(option_string_bytes(&record.protocol))
        .saturating_add(option_string_bytes(&record.cipher_suite))
        .saturating_add(option_string_bytes(&record.alpn))
        .saturating_add(option_string_bytes(&record.error))
}

fn option_string_bytes(value: &Option<String>) -> usize {
    value.as_ref().map(String::capacity).unwrap_or(0)
}

fn vector_storage_bytes<T>(capacity: usize) -> usize {
    capacity.saturating_mul(size_of::<T>())
}
