use super::*;

pub(in crate::proxy) fn handle_http_head<W: WsIo + Send>(
    client: &mut W,
    mut head: http::RequestHead,
    state: &SharedState,
    mut connection: HttpConnectionInput,
) -> io::Result<ClientPersistence> {
    let deadline = RequestDeadline::new(state.config.request_total_timeout)?;
    let rules = state.rules.snapshot();
    if head.body.has_body() && request_expects_continue(&head.request) {
        client.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        client.flush()?;
        connection.initial_flags.push("expect-continue".to_string());
    }

    if !head.body.has_body() {
        return handle_http_stream_inner(
            client,
            state,
            HttpStreamInput {
                request: head.request,
                rules,
                connection,
                deadline,
                request_body: None,
                request_body_rules_skipped: false,
            },
        );
    }

    let full_url = absolute_url_for(&head.request, connection.https_authority.as_deref())?;
    let planning_meta = RequestMeta {
        method: head.request.method.clone(),
        url: full_url.clone(),
        headers: head.request.headers.clone(),
        body: Vec::new(),
        client_ip: Some(connection.peer.clone()),
        server_ip: literal_ip_from_url(&full_url),
        template: Default::default(),
    };
    let body_required = rules.compiled.request_body_required(&planning_meta);

    let request_body = match read_request_body_bounded_with_deadline(
        client,
        head.body,
        state.config.body_buffer_limit,
        state.config.max_header_size,
        state.config.max_header_count,
        deadline,
    )? {
        http::BoundedRequestBody::Complete { body, trailers } => {
            head.request.body = body;
            head.request.trailers = trailers;
            None
        }
        http::BoundedRequestBody::Overflow { prefix, reader } => Some(
            StreamingRequestBody::overflow(prefix, reader, body_required),
        ),
    };
    let request_body_rules_skipped = request_body.is_some();

    handle_http_stream_inner(
        client,
        state,
        HttpStreamInput {
            request: head.request,
            rules,
            connection,
            deadline,
            request_body,
            request_body_rules_skipped,
        },
    )
}
