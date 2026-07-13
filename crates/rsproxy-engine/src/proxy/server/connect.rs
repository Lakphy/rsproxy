use super::connect_policy::ConnectDecision;
use super::probe::ConnectProtocol;
use super::*;

pub(super) fn handle_connect(
    mut client: TcpStream,
    req: RawRequest,
    state: SharedState,
    peer: String,
) -> io::Result<()> {
    let target = req.target.clone();
    let url = format!("tunnel://{target}");
    let meta = RequestMeta {
        method: "CONNECT".to_string(),
        url: url.clone(),
        headers: req.headers.clone(),
        body: Vec::new(),
        client_ip: Some(peer.clone()),
        server_ip: literal_ip_from_url(&url),
        template: Default::default(),
    };
    let resolved = state.rules.snapshot().compiled.resolve(&meta);
    let mut session = Session::new(
        SessionKind::Tunnel,
        "CONNECT".to_string(),
        target.clone(),
        peer,
    );
    session.req_headers = req.headers;
    session.req_trailers = req.trailers;
    if !session.req_trailers.is_empty() {
        session.flags.push("req-trailers".to_string());
    }
    session.matched_rules = resolved.matched_rules;
    session.flags.push("tunnel".to_string());
    apply_trace_tags(&mut session, &resolved.actions, &meta, &state);
    let hidden = trace_hidden(&resolved.actions);
    let deadline = RequestDeadline::new(state.config.request_total_timeout)?;

    match connect_policy::decide(&state, &target, &resolved.actions) {
        ConnectDecision::Passthrough { flags } => {
            session.flags.extend(flags);
            handle_passthrough(PassthroughInput {
                client,
                target: &target,
                actions: &resolved.actions,
                meta: &meta,
                state: &state,
                session,
                hidden,
                deadline,
                response_established: false,
            })
        }
        ConnectDecision::Inspect { host, flags } => {
            session.flags.extend(flags.clone());
            client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
            client.flush()?;
            let probe_timeout = deadline
                .remaining()?
                .min(state.config.connect_probe_timeout);
            let protocol = match probe::detect(&mut client, probe_timeout) {
                Ok(protocol) => protocol,
                Err(error) => {
                    session.status = Some(502);
                    session.error = Some(error.to_string());
                    session.flags.push("connect-probe-error".to_string());
                    finish_session(&state, session, hidden);
                    return Ok(());
                }
            };
            if let Err(error) = deadline.remaining() {
                session.status = Some(504);
                session.error = Some(error.to_string());
                session.flags.push("request-timeout".to_string());
                session.flags.push("request-total-timeout".to_string());
                finish_session(&state, session, hidden);
                return Ok(());
            }
            handle_inspected(InspectedInput {
                client,
                target,
                host,
                actions: resolved.actions,
                meta,
                state,
                session,
                hidden,
                deadline,
                protocol,
                initial_flags: flags,
            })
        }
    }
}

struct InspectedInput {
    client: TcpStream,
    target: String,
    host: String,
    actions: Vec<ResolvedAction>,
    meta: RequestMeta,
    state: SharedState,
    session: Session,
    hidden: bool,
    deadline: RequestDeadline,
    protocol: ConnectProtocol,
    initial_flags: Vec<String>,
}

fn handle_inspected(input: InspectedInput) -> io::Result<()> {
    let InspectedInput {
        client,
        target,
        host,
        actions,
        meta,
        state,
        mut session,
        hidden,
        deadline,
        protocol,
        mut initial_flags,
    } = input;
    match protocol {
        ConnectProtocol::Tls => {
            session.flags.push("connect-probe-tls".to_string());
            initial_flags.push("connect-probe-tls".to_string());
            mitm::handle_connect_mitm(client, target, host, state, session, hidden, initial_flags)
        }
        ConnectProtocol::Http => {
            session.flags.push("connect-probe-http".to_string());
            initial_flags.push("connect-probe-http".to_string());
            initial_flags.push("connect-http".to_string());
            let mut client = client;
            inner_http::serve(
                &mut client,
                state,
                target,
                session,
                hidden,
                inner_http::ConnectHttpMode::Plain {
                    first_flags: initial_flags,
                },
            )
        }
        ConnectProtocol::Unknown | ConnectProtocol::Timeout => {
            session.flags.push(
                match protocol {
                    ConnectProtocol::Unknown => "connect-probe-unknown",
                    ConnectProtocol::Timeout => "connect-probe-timeout",
                    _ => unreachable!(),
                }
                .to_string(),
            );
            handle_passthrough(PassthroughInput {
                client,
                target: &target,
                actions: &actions,
                meta: &meta,
                state: &state,
                session,
                hidden,
                deadline,
                response_established: true,
            })
        }
        ConnectProtocol::Closed => {
            session.status = Some(200);
            session.flags.push("connect-probe-closed".to_string());
            finish_session(&state, session, hidden);
            Ok(())
        }
    }
}

struct PassthroughInput<'a> {
    client: TcpStream,
    target: &'a str,
    actions: &'a [ResolvedAction],
    meta: &'a RequestMeta,
    state: &'a SharedState,
    session: Session,
    hidden: bool,
    deadline: RequestDeadline,
    response_established: bool,
}

fn handle_passthrough(input: PassthroughInput<'_>) -> io::Result<()> {
    let PassthroughInput {
        mut client,
        target,
        actions,
        meta,
        state,
        mut session,
        hidden,
        deadline,
        response_established,
    } = input;
    begin_session_trace_if_visible(state, &mut session, hidden);
    let mut trace_abort = TraceAbortGuard::new(state, session.id);
    let tunnel_url = UrlParts::parse(&format!("tunnel://{target}"))
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let route = match upstream_route(&tunnel_url, actions, meta, state) {
        Ok(route) => route,
        Err(error) => {
            return finish_upstream_error(ConnectErrorInput {
                client: &mut client,
                target,
                state,
                session,
                hidden,
                timings: NetworkTimings::default(),
                error,
                response_established,
                trace_abort: &mut trace_abort,
            });
        }
    };
    if !route.is_direct() {
        session.flags.push("upstream".to_string());
    }
    session.upstream = Some(route.tunnel_session_label());

    let mut timings = NetworkTimings::default();
    match connect_tunnel_upstream(&route, state, &mut timings, deadline) {
        Ok(upstream) => {
            if !response_established {
                client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")?;
                client.flush()?;
            }
            session.status = Some(200);
            if session.id != 0 {
                state.trace.emit(rsproxy_trace::TraceEvent::Response {
                    id: session.id,
                    status: session.status,
                    headers: Vec::new(),
                    trailers: Vec::new(),
                });
            }
            let trace = TunnelTrace::new(state.trace.clone(), session.id);
            let (up, down) = tunnel_copy(client, upstream, trace);
            session.request_bytes = up;
            session.response_bytes = down;
            session.dns_ms = timings.dns_ms;
            session.connect_ms = timings.connect_ms;
            if finish_session(state, session, hidden) {
                trace_abort.disarm();
            }
            Ok(())
        }
        Err(error) => finish_upstream_error(ConnectErrorInput {
            client: &mut client,
            target,
            state,
            session,
            hidden,
            timings,
            error,
            response_established,
            trace_abort: &mut trace_abort,
        }),
    }
}

struct ConnectErrorInput<'a> {
    client: &'a mut TcpStream,
    target: &'a str,
    state: &'a SharedState,
    session: Session,
    hidden: bool,
    timings: NetworkTimings,
    error: io::Error,
    response_established: bool,
    trace_abort: &'a mut TraceAbortGuard,
}

fn finish_upstream_error(input: ConnectErrorInput<'_>) -> io::Result<()> {
    let ConnectErrorInput {
        client,
        target,
        state,
        mut session,
        hidden,
        timings,
        error,
        response_established,
        trace_abort,
    } = input;
    let dns_timed_out = is_upstream_dns_timeout(&error);
    let connect_timed_out = is_upstream_tcp_connect_timeout(&error);
    let request_timed_out = is_request_total_timeout(&error);
    let timed_out = dns_timed_out || connect_timed_out || request_timed_out;
    let status = if timed_out { 504 } else { 502 };
    session.status = Some(status);
    session.error = Some(error.to_string());
    session.dns_ms = timings.dns_ms;
    session.connect_ms = timings.connect_ms;
    if timed_out {
        session.flags.push("upstream-timeout".to_string());
    }
    if dns_timed_out {
        session.flags.push("upstream-dns-timeout".to_string());
    }
    if connect_timed_out {
        session
            .flags
            .push("upstream-tcp-connect-timeout".to_string());
    }
    if request_timed_out {
        session.flags.push("request-timeout".to_string());
        session.flags.push("request-total-timeout".to_string());
    }
    if response_established {
        session
            .flags
            .push("connect-upstream-failed-after-200".to_string());
    } else {
        let _ = http::write_response(
            client,
            status,
            http::reason_phrase(status),
            &[("Content-Type".to_string(), "text/plain".to_string())],
            format!("connect {target}: {error}\n").as_bytes(),
        );
    }
    if finish_session(state, session, hidden) {
        trace_abort.disarm();
    }
    Ok(())
}

fn finish_session(state: &SharedState, mut session: Session, hidden: bool) -> bool {
    begin_session_trace_if_visible(state, &mut session, hidden);
    session.finish();
    record_session_if_visible(state, session, hidden)
}
