use super::*;

pub(in crate::proxy) fn finish_buffered_response<W: WsIo + Send>(
    client: &mut W,
    context: &ResponseContext<'_>,
    response: BufferedResponse,
) -> io::Result<ForwardResult> {
    let BufferedResponse {
        mut head,
        mut body,
        trailers: mut response_trailers,
        matched_rules: response_matched_rules,
        actions: response_actions,
        protocol,
        pool_wait_ms,
        request_send_ms,
        response_receive_ms,
    } = response;
    let req = context.request;
    let meta = context.meta;
    let state = context.state;
    let trace_id = context.trace_id;
    let upstream_addr = context.upstream_addr.clone();
    let client_connection = context.client_connection;
    let deadline = context.deadline;
    let mut response_headers = head.headers.clone();
    apply_response_actions(
        &mut head,
        &mut response_headers,
        &mut body,
        meta,
        &response_actions,
        state,
    )?;
    apply_response_trailer_actions(&mut response_trailers, meta, &response_actions, state)?;
    deadline.remaining()?;
    if req.version.eq_ignore_ascii_case("HTTP/1.0") {
        head.version = "HTTP/1.0".to_string();
        response_trailers.clear();
    }
    strip_hop_by_hop_headers(&mut response_headers);
    if response_trailers.is_empty() {
        update_body_headers(&mut response_headers, body.len());
    } else {
        prepare_trailer_headers(&mut head, &mut response_headers, &response_trailers);
    }

    for item in &response_actions {
        if let Action::Delay {
            phase: Phase::Res,
            millis,
        } = item.action
        {
            deadline.sleep(Duration::from_millis(millis))?;
        }
    }

    if trace_id != 0 {
        state.trace.emit(rsproxy_trace::TraceEvent::Response {
            id: trace_id,
            status: Some(head.status),
            headers: response_headers.clone(),
            trailers: response_trailers.clone(),
        });
    }

    if response_trailers.is_empty() {
        let throttle = throttle_bps(&response_actions, Phase::Res);
        if throttle.is_none() {
            let reason = if head.reason.is_empty() {
                http::reason_phrase(head.status)
            } else {
                &head.reason
            };
            http::write_response_with_version_and_connection(
                client,
                &head.version,
                head.status,
                reason,
                &response_headers,
                &body,
                client_connection.keep_alive(),
            )?;
        } else {
            http::write_response_head_with_connection(
                client,
                &head,
                &response_headers,
                client_connection.keep_alive(),
            )?;
            write_maybe_throttled(client, &body, throttle)?;
        }
    } else {
        write_chunked_response(
            client,
            &head,
            &response_headers,
            &body,
            &response_trailers,
            throttle_bps(&response_actions, Phase::Res),
            client_connection,
        )?;
    }
    let frames = if is_sse_response(&response_headers) {
        sse_frames(&body)
    } else {
        Vec::new()
    };
    let kind = if frames.is_empty() {
        None
    } else {
        Some(SessionKind::Sse)
    };
    let res_trace_body_limit = trace_body_limit_for_headers(&state.config, &response_headers);
    let body_head = body.iter().copied().take(res_trace_body_limit).collect();
    Ok(ForwardResult {
        status: head.status,
        upstream: upstream_addr,
        request_bytes: req.body.len() as u64,
        request_body_head: None,
        request_trailers: None,
        response_bytes: body.len() as u64,
        res_headers: response_headers,
        res_trailers: response_trailers,
        body_head,
        frames,
        kind,
        response_matched_rules,
        response_actions,
        protocol,
        client_connection,
        pool_wait_ms,
        request_send_ms,
        response_receive_ms,
        flags: Vec::new(),
        error: None,
    })
}
