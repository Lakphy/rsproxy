use super::*;

mod completion;
mod pending;
mod session;

use completion::{ForwardErrorInput, RequestTimeoutInput};
use completion::{apply_forward_result, finish_request_total_timeout, handle_forward_error};

pub(super) use pending::handle_http_head;

pub(in crate::proxy) struct HttpConnectionInput {
    pub peer: String,
    pub https_authority: Option<String>,
    pub plain_client_clone: Option<TcpStream>,
    pub initial_tls: Vec<TlsRecord>,
    pub started_ms_override: Option<u64>,
    pub initial_flags: Vec<String>,
    pub client_connection: ClientPersistence,
}

pub(in crate::proxy) struct HttpStreamInput {
    pub request: RawRequest,
    pub rules: Arc<crate::rule_store::RuleSnapshot>,
    pub connection: HttpConnectionInput,
    pub deadline: RequestDeadline,
    pub request_body: Option<StreamingRequestBody>,
    pub request_body_rules_skipped: bool,
}

#[cfg(test)]
pub(super) fn handle_http_stream<W: WsIo + Send>(
    client: &mut W,
    req: RawRequest,
    state: SharedState,
    connection: HttpConnectionInput,
) -> io::Result<ClientPersistence> {
    let deadline = RequestDeadline::new(state.config.request_total_timeout)?;
    let rules = state.rules.snapshot();
    handle_http_stream_inner(
        client,
        &state,
        HttpStreamInput {
            request: req,
            rules,
            connection,
            deadline,
            request_body: None,
            request_body_rules_skipped: false,
        },
    )
}

pub(super) fn handle_http_stream_inner<W: WsIo + Send>(
    client: &mut W,
    state: &SharedState,
    input: HttpStreamInput,
) -> io::Result<ClientPersistence> {
    let HttpStreamInput {
        request: mut req,
        rules,
        connection,
        deadline,
        request_body,
        request_body_rules_skipped,
    } = input;
    let HttpConnectionInput {
        peer,
        https_authority,
        plain_client_clone,
        initial_tls,
        started_ms_override,
        initial_flags,
        client_connection,
    } = connection;
    let full_url = absolute_url_for(&req, https_authority.as_deref())?;
    let meta = RequestMeta {
        method: req.method.clone(),
        url: full_url.clone(),
        headers: req.headers.clone(),
        body: req.body.clone(),
        client_ip: Some(peer.clone()),
        server_ip: literal_ip_from_url(&full_url),
        template: Default::default(),
    };
    let resolved = if request_body_rules_skipped {
        rules.compiled.resolve_without_request_body(&meta)
    } else {
        rules.compiled.resolve(&meta)
    };
    let has_streaming_request = request_body.is_some();
    let short_circuit_connection = if has_streaming_request {
        ClientPersistence::Close
    } else {
        client_connection
    };
    let (mut session, mut hidden) = session::begin(session::SessionInput {
        req: &req,
        full_url: &full_url,
        peer,
        initial_tls,
        started_ms_override,
        initial_flags,
        resolved: &resolved,
        meta: &meta,
        state,
        is_mitm: https_authority.is_some(),
    });
    let mut trace_abort = TraceAbortGuard::new(state, session.id);

    for item in &resolved.actions {
        if let Action::Delay {
            phase: Phase::Req,
            millis,
        } = item.action
            && let Err(error) = deadline.sleep(Duration::from_millis(millis))
        {
            return finish_request_total_timeout(
                client,
                RequestTimeoutInput {
                    req: &req,
                    state,
                    session,
                    hidden,
                    client_connection: short_circuit_connection,
                    error,
                    trace_abort: &mut trace_abort,
                },
            );
        }
    }

    let effective_url = apply_url_actions(&full_url, &meta, &resolved.actions, state)?;
    if effective_url != full_url {
        session.flags.push("url-rewrite".to_string());
        session.url = effective_url.clone();
        if resolved
            .actions
            .iter()
            .any(|item| matches!(item.action, Action::MapRemote(_)))
        {
            session.flags.push("map-remote".to_string());
        }
    }
    if upstream_mtls_enabled(&effective_url, &resolved.actions, &meta, state) {
        session.flags.push("upstream-mtls".to_string());
    }
    apply_upstream_tls_policy_flags(
        &mut session,
        &effective_url,
        &resolved.actions,
        &meta,
        state,
    );
    if has_streaming_request {
        apply_streaming_request_actions(&mut req, &meta, &resolved.actions, state)?;
    } else {
        apply_request_actions(&mut req, &meta, &resolved.actions, state)?;
    }
    session.req_headers = req.headers.clone();
    session.req_trailers = req.trailers.clone();
    session.request_bytes = req.body.len() as u64;
    let req_trace_body_limit = trace_body_limit_for_headers(&state.config, &req.headers);
    session.req_body_head = req
        .body
        .iter()
        .copied()
        .take(req_trace_body_limit)
        .collect();
    trace_abort.emit_request(&session);

    if let Err(error) = deadline.remaining() {
        return finish_request_total_timeout(
            client,
            RequestTimeoutInput {
                req: &req,
                state,
                session,
                hidden,
                client_connection: short_circuit_connection,
                error,
                trace_abort: &mut trace_abort,
            },
        );
    }

    if let Some((status, rule)) = first_status(&resolved.actions) {
        session.status = Some(status);
        session.flags.push("status".to_string());
        let body = if http::status_can_send_content(status) {
            format!(
                "rsproxy status({status}) from {}:{}\n",
                rule.rule.group, rule.rule.line
            )
        } else {
            String::new()
        };
        let body_allowed = http::response_can_send_content(&req.method, status);
        session.response_bytes = if body_allowed { body.len() as u64 } else { 0 };
        session.res_body_head = body
            .as_bytes()
            .iter()
            .copied()
            .take(if body_allowed {
                state.config.trace_body_limit
            } else {
                0
            })
            .collect();
        let mut headers = Vec::new();
        if http::status_can_send_content(status) {
            headers.push(("Content-Type".to_string(), "text/plain".to_string()));
        }
        apply_client_connection_flag(&mut session, &req.version, short_circuit_connection);
        session.finish();
        if req.method.eq_ignore_ascii_case("HEAD") && http::status_can_send_content(status) {
            http::set_header(&mut headers, "Content-Length", body.len().to_string());
            http::write_response_head_with_connection(
                &mut *client,
                &http::RawResponseHead {
                    version: client_response_version(&req.version).to_string(),
                    status,
                    reason: http::reason_phrase(status).to_string(),
                    headers: Vec::new(),
                },
                &headers,
                short_circuit_connection.keep_alive(),
            )?;
            client.flush()?;
        } else {
            http::write_response_with_version_and_connection(
                &mut *client,
                client_response_version(&req.version),
                status,
                http::reason_phrase(status),
                &headers,
                if body_allowed { body.as_bytes() } else { b"" },
                short_circuit_connection.keep_alive(),
            )?;
        }
        if record_session_if_visible(state, session, hidden) {
            trace_abort.disarm();
        }
        return Ok(short_circuit_connection);
    }

    if let Some((url, code)) = first_redirect(&resolved.actions, &meta, state)? {
        session.status = Some(code);
        session.flags.push("redirect".to_string());
        apply_client_connection_flag(&mut session, &req.version, short_circuit_connection);
        session.finish();
        http::write_response_with_version_and_connection(
            &mut *client,
            client_response_version(&req.version),
            code,
            http::reason_phrase(code),
            &[("Location".to_string(), url)],
            b"",
            short_circuit_connection.keep_alive(),
        )?;
        if record_session_if_visible(state, session, hidden) {
            trace_abort.disarm();
        }
        return Ok(short_circuit_connection);
    }

    if let Some(mock) = first_mock(&resolved.actions, &meta, state)? {
        session.status = Some(mock.status);
        session.flags.push("mock".to_string());
        let body_allowed = http::response_can_send_content(&req.method, mock.status);
        session.response_bytes = if body_allowed {
            mock.body.len() as u64
        } else {
            0
        };
        let mut response_headers = mock.headers.clone();
        if http::status_can_send_content(mock.status) {
            http::set_header(
                &mut response_headers,
                "Content-Length",
                mock.body.len().to_string(),
            );
        }
        session.res_headers = response_headers.clone();
        let res_trace_body_limit =
            trace_body_limit_for_headers(&state.config, &session.res_headers);
        session.res_body_head = mock
            .body
            .iter()
            .copied()
            .take(if body_allowed {
                res_trace_body_limit
            } else {
                0
            })
            .collect();
        apply_client_connection_flag(&mut session, &req.version, short_circuit_connection);
        session.finish();
        if req.method.eq_ignore_ascii_case("HEAD") && http::status_can_send_content(mock.status) {
            http::write_response_head_with_connection(
                &mut *client,
                &http::RawResponseHead {
                    version: client_response_version(&req.version).to_string(),
                    status: mock.status,
                    reason: mock.reason.clone(),
                    headers: Vec::new(),
                },
                &response_headers,
                short_circuit_connection.keep_alive(),
            )?;
            client.flush()?;
        } else {
            http::write_response_with_version_and_connection(
                &mut *client,
                client_response_version(&req.version),
                mock.status,
                &mock.reason,
                &response_headers,
                if body_allowed { &mock.body } else { b"" },
                short_circuit_connection.keep_alive(),
            )?;
        }
        if record_session_if_visible(state, session, hidden) {
            trace_abort.disarm();
        }
        return Ok(short_circuit_connection);
    }

    let planned_upstream = planned_upstream_addr(&effective_url, &resolved.actions, &meta, state);
    let mut network_timings = NetworkTimings::default();
    let response_connection = match forward(
        &mut *client,
        ForwardInput {
            request: &req,
            full_url: &effective_url,
            meta: &meta,
            actions: &resolved.actions,
            state,
            trace_id: session.id,
            rules: &rules.compiled,
            plain_client_clone,
            client_connection,
            deadline,
            request_body,
            request_body_rules_skipped,
        },
        &mut session.tls,
        &mut network_timings,
    ) {
        Ok(result) => {
            network_timings.request_send_ms = result.request_send_ms;
            network_timings.response_receive_ms = result.response_receive_ms;
            apply_forward_result(&mut session, &mut hidden, &meta, state, result)
        }
        Err(err) => handle_forward_error(
            &mut *client,
            ForwardErrorInput {
                req: &req,
                state,
                session: &mut session,
                planned_upstream,
                network_timings: &mut network_timings,
                has_streaming_request,
                client_connection,
                error: err,
            },
        ),
    };
    session.dns_ms = network_timings.dns_ms;
    session.connect_ms = network_timings.connect_ms;
    session.ttfb_ms = network_timings.ttfb_ms;
    session.request_send_ms = network_timings.request_send_ms;
    session.response_receive_ms = network_timings.response_receive_ms;
    apply_client_connection_flag(&mut session, &req.version, response_connection);
    session.finish();
    if record_session_if_visible(state, session, hidden) {
        trace_abort.disarm();
    }
    Ok(response_connection)
}
