use super::*;

pub(super) fn apply_client_connection_flag(
    session: &mut Session,
    request_version: &str,
    connection: ClientPersistence,
) {
    if !request_version.eq_ignore_ascii_case("HTTP/1.1")
        && !request_version.eq_ignore_ascii_case("HTTP/1.0")
    {
        return;
    }
    session.flags.push(match connection {
        ClientPersistence::KeepAlive => "h1-client-keepalive".to_string(),
        ClientPersistence::Close => "h1-client-close".to_string(),
    });
}

pub(super) fn apply_upstream_pool_error_flags(session: &mut Session, error: &io::Error) {
    let message = error.to_string();
    if message.starts_with("upstream_h1 ") {
        session.flags.push("h1-upstream".to_string());
        if is_h1_pool_wait_timeout(error) {
            session
                .flags
                .push("h1-upstream-pool-wait-timeout".to_string());
            return;
        }
        session
            .flags
            .push(if message.starts_with("upstream_h1 pool_hit ") {
                "h1-upstream-pool-hit".to_string()
            } else {
                "h1-upstream-pool-miss".to_string()
            });
        return;
    }
    if !message.starts_with("upstream_h2 ") {
        return;
    }
    session.flags.push("h2-upstream".to_string());
    if is_h2_pool_wait_timeout(error) {
        session
            .flags
            .push("h2-upstream-pool-wait-timeout".to_string());
        return;
    }
    let negotiated_during_request = session
        .tls
        .iter()
        .any(|record| record.phase == "upstream_tls" && record.alpn.as_deref() == Some("h2"));
    session.flags.push(if negotiated_during_request {
        "h2-upstream-pool-miss".to_string()
    } else {
        "h2-upstream-pool-hit".to_string()
    });
}

pub(super) fn is_h1_pool_wait_timeout(error: &io::Error) -> bool {
    error
        .to_string()
        .starts_with("upstream_h1 pool_wait: timeout after ")
}

pub(super) fn is_h2_pool_wait_timeout(error: &io::Error) -> bool {
    error
        .to_string()
        .starts_with("upstream_h2 pool_wait: timeout after ")
}

pub(super) fn is_upstream_tls_handshake_timeout(error: &io::Error) -> bool {
    error
        .to_string()
        .starts_with("stage=tls_handshake: timeout after ")
}

pub(super) fn is_upstream_tcp_connect_timeout(error: &io::Error) -> bool {
    error
        .to_string()
        .starts_with("stage=connect: timeout after ")
}

pub(super) fn is_upstream_dns_timeout(error: &io::Error) -> bool {
    error.to_string().starts_with("stage=dns: timeout after ")
}

pub(super) fn is_upstream_ttfb_timeout(error: &io::Error) -> bool {
    if error.kind() != io::ErrorKind::TimedOut {
        return false;
    }
    let message = error.to_string();
    message.starts_with("stage=ttfb: timeout after ")
        || message.starts_with("upstream_h2 ttfb: timeout after ")
        || (message.starts_with("upstream_h1 pool_") && message.contains(" ttfb: timeout after "))
}

pub(super) fn merge_matched_rules(existing: &mut Vec<MatchedRule>, additional: Vec<MatchedRule>) {
    let mut merged = Vec::with_capacity(existing.len() + additional.len());
    for rule in existing.drain(..) {
        if !additional.iter().any(|seen| same_rule(seen, &rule)) {
            merged.push(rule);
        }
    }
    for rule in additional {
        if !merged.iter().any(|seen| same_rule(seen, &rule)) {
            merged.push(rule);
        }
    }
    *existing = merged;
}

pub(super) fn same_rule(left: &MatchedRule, right: &MatchedRule) -> bool {
    left.group == right.group && left.line == right.line && left.raw == right.raw
}

pub(super) fn trace_hidden(actions: &[ResolvedAction]) -> bool {
    actions
        .iter()
        .any(|item| matches!(item.action, Action::Hide))
}

pub(super) fn apply_trace_tags(
    session: &mut Session,
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) {
    for item in actions {
        let Action::Tag(value) = &item.action else {
            continue;
        };
        let Ok(tag) = resolve_value_text(value, item, meta, state) else {
            continue;
        };
        let tag = tag.trim();
        if tag.is_empty() {
            continue;
        }
        let flag = format!("tag:{tag}");
        if !session.flags.iter().any(|seen| seen == &flag) {
            session.flags.push(flag);
        }
    }
}

pub(super) fn record_session_if_visible(
    state: &SharedState,
    mut session: Session,
    hidden: bool,
) -> bool {
    if let Some(error) = session.error.as_deref() {
        tracing::warn!(
            event = "proxy_session_failed",
            session_id = session.id,
            kind = ?session.kind,
            method = %session.method,
            url = %session.url,
            status = ?session.status,
            duration_ms = session.duration_ms,
            trace_hidden = hidden,
            error = %error,
            "proxy session failed"
        );
    } else {
        tracing::debug!(
            event = "proxy_session_finished",
            session_id = session.id,
            kind = ?session.kind,
            method = %session.method,
            url = %session.url,
            status = ?session.status,
            duration_ms = session.duration_ms,
            trace_hidden = hidden,
            "proxy session finished"
        );
    }
    if hidden {
        return if session.id == 0 {
            true
        } else {
            state.trace.abort(session.id)
        };
    }
    begin_session_trace_if_visible(state, &mut session, false);
    state.trace.finish(session)
}

pub(super) fn begin_session_trace_if_visible(
    state: &SharedState,
    session: &mut Session,
    hidden: bool,
) {
    if hidden || session.id != 0 {
        return;
    }
    session.id = state
        .trace
        .start(rsproxy_trace::SessionStart::from_session(session));
    state.trace.emit(rsproxy_trace::TraceEvent::Request {
        id: session.id,
        method: Some(session.method.clone()),
        url: Some(session.url.clone()),
        headers: session.req_headers.clone(),
        trailers: session.req_trailers.clone(),
        matched_rules: session.matched_rules.clone(),
    });
}

pub(super) struct TraceAbortGuard {
    trace: rsproxy_trace::TraceStore,
    id: u64,
}

impl TraceAbortGuard {
    pub(super) fn new(state: &SharedState, id: u64) -> Self {
        Self {
            trace: state.trace.clone(),
            id,
        }
    }

    pub(super) fn disarm(&mut self) {
        self.id = 0;
    }

    pub(super) fn emit_request(&self, session: &Session) {
        if self.id != 0 {
            self.trace.emit(rsproxy_trace::TraceEvent::Request {
                id: self.id,
                method: Some(session.method.clone()),
                url: Some(session.url.clone()),
                headers: session.req_headers.clone(),
                trailers: session.req_trailers.clone(),
                matched_rules: session.matched_rules.clone(),
            });
        }
    }
}

impl Drop for TraceAbortGuard {
    fn drop(&mut self) {
        if self.id != 0 {
            self.trace.abort(self.id);
        }
    }
}

pub(super) struct BodyTraceEmitter<'a> {
    store: &'a rsproxy_trace::TraceStore,
    id: u64,
    direction: rsproxy_trace::BodyDirection,
    remaining: usize,
}

impl<'a> BodyTraceEmitter<'a> {
    pub(super) fn new(
        store: &'a rsproxy_trace::TraceStore,
        id: u64,
        direction: rsproxy_trace::BodyDirection,
        limit: usize,
    ) -> Self {
        Self {
            store,
            id,
            direction,
            remaining: limit,
        }
    }

    pub(super) fn observe_slice(&mut self, data: &[u8]) {
        let captured = data.len().min(self.remaining);
        let preview = bytes::Bytes::copy_from_slice(&data[..captured]);
        self.emit(preview, data.len());
    }

    pub(super) fn observe_bytes(&mut self, data: &bytes::Bytes) {
        let captured = data.len().min(self.remaining);
        let preview = if captured == 0 {
            bytes::Bytes::new()
        } else {
            data.slice(..captured)
        };
        self.emit(preview, data.len());
    }

    fn emit(&mut self, data: bytes::Bytes, observed: usize) {
        self.remaining = self.remaining.saturating_sub(data.len());
        self.store.emit(rsproxy_trace::TraceEvent::BodyChunk {
            id: self.id,
            direction: self.direction,
            data,
            observed_bytes: observed as u64,
        });
    }
}
