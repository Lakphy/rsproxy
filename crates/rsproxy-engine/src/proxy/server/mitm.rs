use super::*;

pub(super) fn handle_connect_mitm(
    mut client: TcpStream,
    target: String,
    host: String,
    state: SharedState,
    mut connect_session: Session,
    hidden: bool,
    mut initial_flags: Vec<String>,
) -> io::Result<()> {
    let (config, cache_hit) = match mitm_server_config(&state, &host) {
        Ok(result) => result,
        Err(error) => {
            connect_session.status = Some(502);
            connect_session.error = Some(error.to_string());
            connect_session.flags.push("mitm-setup-failed".to_string());
            finish_session(&state, connect_session, hidden);
            return Ok(());
        }
    };
    let cache_flag = if cache_hit {
        "mitm-cert-cache-hit"
    } else {
        "mitm-cert-cache-miss"
    };
    connect_session.flags.push(cache_flag.to_string());
    initial_flags.push(cache_flag.to_string());

    let mut connection = ServerConnection::new(config).map_err(io::Error::other)?;
    let handshake_started = rsproxy_trace::now_millis();
    let handshake_ms = match complete_client_tls_handshake(
        &mut connection,
        &mut client,
        state.config.client_tls_handshake_timeout,
    ) {
        Ok(handshake_ms) => handshake_ms,
        Err(error) => {
            record_handshake_failure(
                &state,
                &host,
                &mut connect_session,
                handshake_started,
                error,
            );
            finish_session(&state, connect_session, hidden);
            return Ok(());
        }
    };
    state
        .mitm_failures
        .lock()
        .expect("MITM failure cache lock poisoned")
        .clear(&host);
    let client_tls = server_tls_record("client_mitm_tls", &host, handshake_ms, &connection);
    let mut tls = StreamOwned::new(connection, client);
    if tls.conn.alpn_protocol() == Some(H2_ALPN) {
        return h2_bridge::serve_h2_mitm(
            tls,
            state,
            connect_session.client,
            target,
            client_tls,
            initial_flags,
        );
    }
    inner_http::serve(
        &mut tls,
        state,
        target,
        connect_session,
        hidden,
        inner_http::ConnectHttpMode::Mitm {
            client_tls,
            first_flags: initial_flags,
        },
    )
}

fn record_handshake_failure(
    state: &SharedState,
    host: &str,
    session: &mut Session,
    handshake_started: u64,
    error: io::Error,
) {
    let timed_out = is_client_tls_handshake_timeout(&error);
    session.status = Some(if timed_out { 408 } else { 502 });
    session.error = Some(error.to_string());
    session.tls.push(failed_tls_record(
        "client_mitm_tls",
        host,
        handshake_started,
        &error,
    ));
    if timed_out {
        session.flags.push("client-timeout".to_string());
        session
            .flags
            .push("client-tls-handshake-timeout".to_string());
        return;
    }
    if state.config.strict_mitm {
        session.flags.push("mitm-fallback-disabled".to_string());
    } else if state
        .mitm_failures
        .lock()
        .expect("MITM failure cache lock poisoned")
        .remember(host)
    {
        session.flags.push("mitm-fallback-remembered".to_string());
    }
}

fn finish_session(state: &SharedState, mut session: Session, hidden: bool) {
    begin_session_trace_if_visible(state, &mut session, hidden);
    session.finish();
    let _ = record_session_if_visible(state, session, hidden);
}
