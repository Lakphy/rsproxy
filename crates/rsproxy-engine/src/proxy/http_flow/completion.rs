use super::*;

pub(super) fn apply_forward_result(
    session: &mut Session,
    hidden: &mut bool,
    meta: &RequestMeta,
    state: &SharedState,
    result: ForwardResult,
) -> ClientPersistence {
    let response_connection = result.client_connection;
    session.status = Some(result.status);
    session.upstream = Some(result.upstream);
    session.request_bytes = result.request_bytes;
    if let Some(body_head) = result.request_body_head {
        session.req_body_head = body_head;
    }
    if let Some(trailers) = result.request_trailers {
        session.req_trailers = trailers;
        if !session.req_trailers.is_empty()
            && !session.flags.iter().any(|flag| flag == "req-trailers")
        {
            session.flags.push("req-trailers".to_string());
        }
    }
    session.response_bytes = result.response_bytes;
    session.res_headers = result.res_headers;
    session.res_trailers = result.res_trailers;
    session.res_body_head = result.body_head;
    session.pool_wait_ms = result.pool_wait_ms;
    session.flags.extend(result.flags);
    session.error = result.error;
    match result.protocol {
        UpstreamProtocol::Http1Pooled { reused_connection } => {
            session.flags.push("h1-upstream".to_string());
            session.flags.push(if reused_connection {
                "h1-upstream-pool-hit".to_string()
            } else {
                "h1-upstream-pool-miss".to_string()
            });
        }
        UpstreamProtocol::Http2 { reused_connection } => {
            session.flags.push("h2-upstream".to_string());
            session.flags.push(if reused_connection {
                "h2-upstream-pool-hit".to_string()
            } else {
                "h2-upstream-pool-miss".to_string()
            });
        }
        UpstreamProtocol::Http1 => {}
    }
    if session.flags.iter().any(|flag| flag == "h2-client") {
        prepare_h2_client_response_headers(&mut session.res_headers, result.status, None);
    }
    merge_matched_rules(&mut session.matched_rules, result.response_matched_rules);
    apply_trace_tags(session, &result.response_actions, meta, state);
    *hidden |= trace_hidden(&result.response_actions);
    if !session.res_trailers.is_empty() {
        session.flags.push("trailers".to_string());
    }
    if let Some(kind) = result.kind {
        session.kind = kind;
        session.frames = result.frames;
        match kind {
            SessionKind::Sse => session.flags.push("sse".to_string()),
            SessionKind::WebSocket => session.flags.push("websocket".to_string()),
            _ => {}
        }
    }
    response_connection
}

pub(super) struct ForwardErrorInput<'a> {
    pub req: &'a RawRequest,
    pub state: &'a SharedState,
    pub session: &'a mut Session,
    pub planned_upstream: Option<String>,
    pub network_timings: &'a mut NetworkTimings,
    pub has_streaming_request: bool,
    pub client_connection: ClientPersistence,
    pub error: io::Error,
}

pub(super) fn handle_forward_error<W: WsIo + Send>(
    client: &mut W,
    input: ForwardErrorInput<'_>,
) -> ClientPersistence {
    let ForwardErrorInput {
        req,
        state,
        session,
        planned_upstream,
        network_timings,
        has_streaming_request,
        client_connection,
        error: err,
    } = input;
    let pool_wait_timeout = if is_h1_pool_wait_timeout(&err) {
        Some(state.config.h1_pool_wait_timeout)
    } else if is_h2_pool_wait_timeout(&err) {
        Some(state.config.h2_pool_wait_timeout)
    } else {
        None
    };
    let dns_timed_out = is_upstream_dns_timeout(&err);
    let tcp_connect_timed_out = is_upstream_tcp_connect_timeout(&err);
    let tls_handshake_timed_out = is_upstream_tls_handshake_timeout(&err);
    let ttfb_timed_out = is_upstream_ttfb_timeout(&err);
    let request_total_timed_out = is_request_total_timeout(&err);
    let client_request_body_error = is_client_request_body_error(&err);
    let status = if client_request_body_error {
        400
    } else if pool_wait_timeout.is_some()
        || dns_timed_out
        || tcp_connect_timed_out
        || tls_handshake_timed_out
        || ttfb_timed_out
        || request_total_timed_out
    {
        504
    } else {
        502
    };
    session.status = Some(status);
    session.upstream = planned_upstream;
    session.error = Some(err.to_string());
    if client_request_body_error {
        session.flags.push("client-request-body-error".to_string());
    }
    if let Some(pool_wait_timeout) = pool_wait_timeout {
        session.pool_wait_ms = pool_wait_timeout.as_millis().min(u64::MAX as u128) as u64;
    }
    if tls_handshake_timed_out {
        session.flags.push("upstream-timeout".to_string());
        session
            .flags
            .push("upstream-tls-handshake-timeout".to_string());
    }
    if dns_timed_out {
        session.flags.push("upstream-timeout".to_string());
        session.flags.push("upstream-dns-timeout".to_string());
    }
    if tcp_connect_timed_out {
        session.flags.push("upstream-timeout".to_string());
        session
            .flags
            .push("upstream-tcp-connect-timeout".to_string());
    }
    if ttfb_timed_out {
        network_timings.ttfb_ms = state
            .config
            .upstream_ttfb_timeout
            .as_millis()
            .min(u64::MAX as u128) as u64;
        session.flags.push("upstream-timeout".to_string());
        session.flags.push("upstream-ttfb-timeout".to_string());
    }
    if request_total_timed_out {
        session.flags.push("upstream-timeout".to_string());
        apply_request_total_timeout_flags(session);
    }
    apply_upstream_pool_error_flags(session, &err);
    let error_connection = if has_streaming_request {
        ClientPersistence::Close
    } else {
        client_connection
    };
    let _ = http::write_response_with_version_and_connection(
        client,
        client_response_version(&req.version),
        status,
        http::reason_phrase(status),
        &[("Content-Type".to_string(), "text/plain".to_string())],
        format!("upstream error: {err}\n").as_bytes(),
        error_connection.keep_alive(),
    );
    error_connection
}

pub(super) struct RequestTimeoutInput<'a> {
    pub req: &'a RawRequest,
    pub state: &'a SharedState,
    pub session: Session,
    pub hidden: bool,
    pub client_connection: ClientPersistence,
    pub error: io::Error,
    pub trace_abort: &'a mut TraceAbortGuard,
}

pub(super) fn finish_request_total_timeout<W: WsIo + Send>(
    client: &mut W,
    input: RequestTimeoutInput<'_>,
) -> io::Result<ClientPersistence> {
    let RequestTimeoutInput {
        req,
        state,
        mut session,
        hidden,
        client_connection,
        error,
        trace_abort,
    } = input;
    session.status = Some(504);
    session.error = Some(error.to_string());
    apply_request_total_timeout_flags(&mut session);
    apply_client_connection_flag(&mut session, &req.version, client_connection);
    session.finish();
    http::write_response_with_version_and_connection(
        client,
        client_response_version(&req.version),
        504,
        http::reason_phrase(504),
        &[("Content-Type".to_string(), "text/plain".to_string())],
        format!("upstream error: {error}\n").as_bytes(),
        client_connection.keep_alive(),
    )?;
    if record_session_if_visible(state, session, hidden) {
        trace_abort.disarm();
    }
    Ok(client_connection)
}

fn apply_request_total_timeout_flags(session: &mut Session) {
    session.flags.push("request-timeout".to_string());
    session.flags.push("request-total-timeout".to_string());
}
