use super::*;

pub(super) enum ConnectHttpMode {
    Mitm {
        client_tls: TlsRecord,
        first_flags: Vec<String>,
    },
    Plain {
        first_flags: Vec<String>,
    },
}

pub(super) fn serve<W: WsIo + Send>(
    client: &mut W,
    state: SharedState,
    target: String,
    mut connect_session: Session,
    hidden: bool,
    mode: ConnectHttpMode,
) -> io::Result<()> {
    let peer = connect_session.client.clone();
    let connect_started_ms = connect_session.started_ms;
    let mut request_index = 0usize;
    loop {
        let mut head = match http::read_request_head(
            &mut *client,
            state.config.max_header_size,
            state.config.max_header_count,
        ) {
            Ok(Some(head)) => head,
            Ok(None) => {
                if request_index == 0 {
                    connect_session.status = Some(200);
                    connect_session.flags.push(mode.empty_flag().to_string());
                    finish_connect_session(&state, connect_session, hidden);
                }
                return Ok(());
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                let status = if error.to_string().contains("limit exceeded") {
                    431
                } else {
                    400
                };
                let _ = http::write_response(
                    &mut *client,
                    status,
                    http::reason_phrase(status),
                    &[("Content-Type".to_string(), "text/plain".to_string())],
                    error.to_string().as_bytes(),
                );
                if request_index == 0 {
                    connect_session.status = Some(status);
                    connect_session.error = Some(stage_error("client_http", error).to_string());
                    finish_connect_session(&state, connect_session, hidden);
                }
                return Ok(());
            }
            Err(error)
                if request_index > 0
                    && matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
            {
                return Ok(());
            }
            Err(error) => {
                if request_index == 0 {
                    connect_session.status = Some(502);
                    connect_session.error =
                        Some(stage_error(mode.read_error_stage(), error).to_string());
                    finish_connect_session(&state, connect_session, hidden);
                }
                return Ok(());
            }
        };
        mode.prepare_head(&mut head, &target);
        let requested_connection = requested_client_connection(&head.request);
        let flags = mode.flags(request_index);
        let initial_tls = mode.tls_records(request_index);
        let request_version = head.request.version.clone();
        let plain_client_clone = if is_websocket_request(&head.request.headers) {
            client.try_clone_plain()?
        } else {
            None
        };
        let response_connection = match handle_http_head(
            &mut *client,
            head,
            &state,
            HttpConnectionInput {
                peer: peer.clone(),
                https_authority: mode.authority(&target),
                plain_client_clone,
                initial_tls,
                started_ms_override: (request_index == 0).then_some(connect_started_ms),
                initial_flags: flags,
                client_connection: requested_connection,
            },
        ) {
            Ok(connection) => connection,
            Err(error) if is_h1_request_input_error(&error) => {
                write_h1_request_input_error(&mut *client, &request_version, &error)?;
                if request_index == 0 {
                    connect_session.status = Some(h1_request_input_error_status(&error));
                    connect_session.error = Some(error.to_string());
                    finish_connect_session(&state, connect_session, hidden);
                }
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        client.flush()?;
        if response_connection == ClientPersistence::Close {
            return Ok(());
        }
        if request_index == 0 {
            client.set_request_read_timeout(Some(CLIENT_KEEPALIVE_IDLE_TIMEOUT))?;
        }
        request_index += 1;
    }
}

impl ConnectHttpMode {
    fn empty_flag(&self) -> &'static str {
        match self {
            Self::Mitm { .. } => "mitm-empty",
            Self::Plain { .. } => "connect-http-empty",
        }
    }

    fn read_error_stage(&self) -> &'static str {
        match self {
            Self::Mitm { .. } => "client_tls",
            Self::Plain { .. } => "client_http",
        }
    }

    fn authority(&self, target: &str) -> Option<String> {
        matches!(self, Self::Mitm { .. }).then(|| target.to_string())
    }

    fn flags(&self, request_index: usize) -> Vec<String> {
        if request_index == 0 {
            return match self {
                Self::Mitm { first_flags, .. } | Self::Plain { first_flags } => first_flags.clone(),
            };
        }
        let mut flags = vec!["h1-client-connection-reused".to_string()];
        flags.push(
            match self {
                Self::Mitm { .. } => "mitm-tunnel-reused",
                Self::Plain { .. } => "connect-http-reused",
            }
            .to_string(),
        );
        flags
    }

    fn tls_records(&self, request_index: usize) -> Vec<TlsRecord> {
        let Self::Mitm { client_tls, .. } = self else {
            return Vec::new();
        };
        let mut record = client_tls.clone();
        if request_index > 0 {
            record.handshake_ms = 0;
        }
        vec![record]
    }

    fn prepare_head(&self, head: &mut http::RequestHead, target: &str) {
        if matches!(self, Self::Plain { .. })
            && head.request.target.starts_with('/')
            && !head.request.target.contains("://")
        {
            head.request.target = format!("http://{target}{}", head.request.target);
        }
    }
}

fn finish_connect_session(state: &SharedState, mut session: Session, hidden: bool) {
    begin_session_trace_if_visible(state, &mut session, hidden);
    session.finish();
    let _ = record_session_if_visible(state, session, hidden);
}
