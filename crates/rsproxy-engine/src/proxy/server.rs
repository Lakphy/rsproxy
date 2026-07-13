use super::*;

mod connect;
mod connect_policy;
mod inner_http;
mod mitm;
mod probe;
mod request;

use connect::handle_connect;
use request::*;

pub(in crate::proxy) use request::{is_h1_request_input_error, write_h1_request_input_error};

/// Accepts proxy connections until the supplied listener fails.
///
/// Each accepted connection runs on an isolated thread; a panic terminates that
/// connection while the accept loop remains available for subsequent clients.
pub fn serve(listener: TcpListener, state: SharedState) -> crate::EngineResult<()> {
    let bound = listener
        .local_addr()
        .map_err(|source| crate::EngineError::Io {
            context: "read proxy listener address".to_string(),
            source,
        })?;
    tracing::info!(
        event = "proxy_listener_bound",
        address = %bound,
        "proxy listener bound"
    );
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = state.clone();
                let peer = stream
                    .peer_addr()
                    .map(|address| address.to_string())
                    .unwrap_or_else(|_| "unknown".to_string());
                thread::spawn(move || {
                    if let Err(err) = handle_client(stream, state) {
                        tracing::warn!(
                            event = "proxy_connection_failed",
                            peer = %peer,
                            error = %err,
                            "proxy connection failed"
                        );
                    }
                });
            }
            Err(err) => tracing::warn!(
                event = "proxy_accept_failed",
                address = %bound,
                error = %err,
                "proxy accept failed"
            ),
        }
    }
    Ok(())
}

pub(super) fn handle_client(mut client: TcpStream, state: SharedState) -> io::Result<()> {
    client.set_nodelay(true)?;
    let peer = client
        .peer_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "-".to_string());
    let mut request_index = 0usize;
    loop {
        let mut head = match http::read_request_head_tcp(
            &mut client,
            state.config.max_header_size,
            state.config.max_header_count,
        ) {
            Ok(Some(head)) => head,
            Ok(None) => return Ok(()),
            Err(err)
                if request_index > 0
                    && matches!(
                        err.kind(),
                        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
                    ) =>
            {
                return Ok(());
            }
            Err(err) if err.kind() == io::ErrorKind::InvalidData => {
                let header_limit = err.to_string().contains("limit exceeded");
                let status = if header_limit { 431 } else { 400 };
                let _ = http::write_response(
                    &mut client,
                    status,
                    http::reason_phrase(status),
                    &[("Content-Type".to_string(), "text/plain".to_string())],
                    err.to_string().as_bytes(),
                );
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        let requested_connection = requested_client_connection(&head.request);

        if !authorize_and_strip_proxy_credentials(
            &mut head.request,
            state.config.proxy_auth.as_deref(),
        ) {
            let response_connection = if head.body.has_body() {
                ClientPersistence::Close
            } else {
                requested_connection
            };
            http::write_response_with_version_and_connection(
                &mut client,
                client_response_version(&head.request.version),
                407,
                "Proxy Authentication Required",
                &[
                    (
                        "Proxy-Authenticate".to_string(),
                        "Basic realm=\"rsproxy\"".to_string(),
                    ),
                    ("Content-Type".to_string(), "text/plain".to_string()),
                ],
                b"proxy authentication required\n",
                response_connection.keep_alive(),
            )?;
            if response_connection == ClientPersistence::Close {
                return Ok(());
            }
            request_index += 1;
            continue;
        }

        if head.request.method.eq_ignore_ascii_case("CONNECT") {
            let Some(req) = collect_connect_request(&mut client, head, &state)? else {
                return Ok(());
            };
            return handle_connect(client, req, state, peer);
        }
        let initial_flags = if request_index > 0 {
            vec!["h1-client-connection-reused".to_string()]
        } else {
            Vec::new()
        };
        let client_clone = if is_websocket_request(&head.request.headers) {
            client.try_clone().ok()
        } else {
            None
        };
        let request_version = head.request.version.clone();
        let response_connection = match handle_http_head(
            &mut client,
            head,
            &state,
            HttpConnectionInput {
                peer: peer.clone(),
                https_authority: None,
                plain_client_clone: client_clone,
                initial_tls: Vec::new(),
                started_ms_override: None,
                initial_flags,
                client_connection: requested_connection,
            },
        ) {
            Ok(connection) => connection,
            Err(error) if is_h1_request_input_error(&error) => {
                write_h1_request_input_error(&mut client, &request_version, &error)?;
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        if response_connection == ClientPersistence::Close {
            return Ok(());
        }
        if request_index == 0 {
            client.set_read_timeout(Some(CLIENT_KEEPALIVE_IDLE_TIMEOUT))?;
        }
        request_index += 1;
    }
}
