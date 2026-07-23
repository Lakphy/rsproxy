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
    let upstream_status = head.status;
    let discarded_upstream_205 = upstream_status == 205 && !body.is_empty();
    let dropped_forbidden_trailer =
        sanitize_upstream_trailers(&mut response_trailers, &head.headers);
    if upstream_status == 205 {
        body.clear();
        response_trailers.clear();
    }
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
    let body_allowed = http::response_can_send_content(&req.method, head.status);
    let status_allows_body = http::status_can_send_content(head.status);
    if !status_allows_body {
        body.clear();
        response_trailers.clear();
        prepare_streaming_body_headers(&mut response_headers);
    } else if req.method.eq_ignore_ascii_case("HEAD") {
        response_trailers.clear();
        http::remove_header(&mut response_headers, "transfer-encoding");
        http::remove_header(&mut response_headers, "trailer");
    }
    if req.version.eq_ignore_ascii_case("HTTP/1.0") {
        head.version = "HTTP/1.0".to_string();
        response_trailers.clear();
    }
    strip_hop_by_hop_headers(&mut response_headers);
    if body_allowed && response_trailers.is_empty() {
        update_body_headers(&mut response_headers, body.len());
    } else if body_allowed {
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

    if !body_allowed {
        http::write_response_head_with_connection(
            client,
            &head,
            &response_headers,
            client_connection.keep_alive(),
        )?;
        client.flush()?;
    } else if response_trailers.is_empty() {
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
    let frames = if body_allowed && is_sse_response(&response_headers) {
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
    let body_head = body
        .iter()
        .copied()
        .take(if body_allowed {
            res_trace_body_limit
        } else {
            0
        })
        .collect();
    Ok(ForwardResult {
        status: head.status,
        upstream: upstream_addr,
        request_bytes: req.body.len() as u64,
        request_body_head: None,
        request_trailers: None,
        response_bytes: if body_allowed { body.len() as u64 } else { 0 },
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
        flags: [
            discarded_upstream_205.then(|| "upstream-205-content-discarded".to_string()),
            dropped_forbidden_trailer.then(|| "forbidden-upstream-trailer-dropped".to_string()),
        ]
        .into_iter()
        .flatten()
        .collect(),
        error: None,
    })
}
