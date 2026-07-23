use super::body_stream::H1BodyStream;
use super::*;

pub(in crate::proxy) fn forward_unpooled<W: WsIo + Send>(
    client: &mut W,
    ctx: &ForwardCtx<'_>,
    plain_client_clone: Option<TcpStream>,
    mut upstream: UpstreamStream,
    network_timings: &mut NetworkTimings,
    request_body: Option<StreamingRequestBody>,
) -> io::Result<ForwardResult> {
    let req = ctx.request;
    let full_url = ctx.full_url;
    let url = ctx.url;
    let meta = ctx.meta;
    let actions = ctx.actions;
    let state = ctx.state;
    let trace_id = ctx.trace_id;
    let rules = ctx.rules;
    let route = ctx.route;
    let upstream_addr = ctx.upstream_addr();
    let websocket_request = ctx.websocket_request();
    let deadline = ctx.deadline;
    let request_body_rules_skipped = ctx.request_body_rules_skipped;
    let mut headers = ctx.headers.to_vec();
    if !websocket_request {
        http::set_header(&mut headers, "Connection", "close".to_string());
    }

    let mut request_summary = None;
    let request_send_started = Instant::now();
    {
        let mut upstream_io = DeadlineIo::new(&mut upstream, deadline);
        write!(
            &mut upstream_io,
            "{} {} {}\r\n",
            req.method,
            if route.uses_absolute_form_for_url(url) {
                full_url.to_string()
            } else {
                url.origin_form()
            },
            upstream_http_version(&req.version)
        )
        .map_err(|err| stage_io_error("request_write", err))?;
        for (name, value) in &headers {
            write!(&mut upstream_io, "{name}: {value}\r\n")
                .map_err(|err| stage_io_error("request_write", err))?;
        }
        write!(&mut upstream_io, "\r\n").map_err(|err| stage_io_error("request_write", err))?;
        if let Some(request_body) = request_body {
            request_summary = Some(relay_request_body(
                client,
                &mut upstream_io,
                request_body,
                RequestRelayConfig {
                    trace_limit: trace_body_limit_for_headers(&state.config, &req.headers),
                    bytes_per_sec: throttle_bps(actions, Phase::Req),
                    max_header_size: state.config.max_header_size,
                    max_header_count: state.config.max_header_count,
                    deadline,
                    trace: (trace_id != 0).then_some((&state.trace, trace_id)),
                },
            )?);
        } else if req.trailers.is_empty() && !req.body.is_empty() {
            write_maybe_throttled_until(
                &mut upstream_io,
                &req.body,
                throttle_bps(actions, Phase::Req),
                deadline,
            )
            .map_err(|err| stage_io_error("request_write", err))?;
        } else if !req.trailers.is_empty() {
            write_chunked_request_until(
                &mut upstream_io,
                &req.body,
                &req.trailers,
                throttle_bps(actions, Phase::Req),
                deadline,
            )
            .map_err(|err| stage_io_error("request_write", err))?;
        }
    }
    let request_send_ms = duration_millis(request_send_started.elapsed());
    network_timings.request_send_ms = Some(request_send_ms);

    let mut head = read_response_head_with_ttfb(
        &mut upstream,
        state.config.max_header_size,
        state.config.max_header_count,
        state.config.upstream_ttfb_timeout,
        deadline,
        network_timings,
    )?;
    let response_receive_started = Instant::now();
    let res_meta = ResponseMeta {
        status: head.status,
        headers: head.headers.clone(),
    };
    let response_resolved = if request_body_rules_skipped {
        rules.resolve_response_without_request_body(meta, &res_meta)
    } else {
        rules.resolve_response(meta, &res_meta)
    };
    let response_matched_rules = response_resolved.matched_rules.clone();
    let response_actions = response_resolved.actions;

    if websocket_request && is_websocket_response(&head.headers, head.status) {
        let request = take_request_observation(req, &mut request_summary);
        return websocket_forward::finish(
            client,
            ctx,
            plain_client_clone,
            upstream,
            websocket_forward::WebSocketUpgrade {
                head,
                matched_rules: response_matched_rules,
                actions: response_actions,
                request_send_ms,
                request,
            },
        );
    }

    if is_sse_response(&head.headers) && can_stream_sse_response(&response_actions) {
        let upstream_response_headers = head.headers.clone();
        let mut response_headers = head.headers.clone();
        let mut body = Vec::new();
        apply_response_actions(
            &mut head,
            &mut response_headers,
            &mut body,
            meta,
            &response_actions,
            state,
        )?;
        strip_hop_by_hop_headers(&mut response_headers);
        prepare_streaming_body_headers(&mut response_headers);

        if trace_id != 0 {
            state.trace.emit(rsproxy_trace::TraceEvent::Response {
                id: trace_id,
                status: Some(head.status),
                headers: response_headers.clone(),
                trailers: Vec::new(),
            });
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

        restore_upstream_timeouts(&mut upstream)?;
        http::write_response_head(client, &head, &response_headers)?;
        let sse_trace_body_limit = trace_body_limit_for_headers(&state.config, &response_headers);
        let mut response_trace = (trace_id != 0).then(|| {
            BodyTraceEmitter::new(
                &state.trace,
                trace_id,
                rsproxy_trace::BodyDirection::Response,
                sse_trace_body_limit,
            )
        });
        let (response_bytes, body_head, frames) = stream_sse_response(
            client,
            &mut upstream,
            &upstream_response_headers,
            sse_trace_body_limit,
            throttle_bps(&response_actions, Phase::Res),
            |data| {
                if let Some(trace) = &mut response_trace {
                    trace.observe_slice(data);
                }
            },
        )
        .map_err(|err| stage_error("response_body", err))?;
        let mut request_observation = take_request_observation(req, &mut request_summary);
        request_observation
            .flags
            .push("response-streamed".to_string());
        let response_receive_ms = duration_millis(response_receive_started.elapsed());
        return Ok(ForwardResult {
            status: head.status,
            upstream: upstream_addr,
            request_bytes: request_observation.bytes,
            request_body_head: request_observation.body_head,
            request_trailers: request_observation.trailers,
            response_bytes,
            res_headers: response_headers,
            res_trailers: Vec::new(),
            body_head,
            frames,
            kind: Some(SessionKind::Sse),
            response_matched_rules,
            response_actions,
            protocol: UpstreamProtocol::Http1,
            client_connection: ClientPersistence::Close,
            pool_wait_ms: 0,
            request_send_ms: Some(request_send_ms),
            response_receive_ms: Some(response_receive_ms),
            flags: request_observation.flags,
            error: None,
        });
    }

    let request_observation = take_request_observation(req, &mut request_summary);
    let response_context = ResponseContext::from_forward(ctx);
    let body_allowed = http::response_has_framed_body(&req.method, head.status);
    let fixed_body_can_coalesce = body_allowed
        && http::header(&head.headers, "trailer").is_none()
        && http::header(&head.headers, "content-length")
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|length| length <= 64 * 1024 && length <= state.config.body_buffer_limit);
    let should_stream = !response_actions_require_body(&response_actions)
        && !req.version.eq_ignore_ascii_case("HTTP/1.0")
        && !fixed_body_can_coalesce;

    let mut result = if should_stream {
        let body = H1BodyStream::new(
            DeadlineIo::new(&mut upstream, deadline),
            &req.method,
            head.status,
            &head.headers,
            state.config.max_header_size,
            state.config.max_header_count,
            response_receive_started,
        )?;
        finish_streaming_response(
            client,
            &response_context,
            StreamingResponse {
                head,
                body,
                prefix: Vec::new(),
                matched_rules: response_matched_rules,
                actions: response_actions,
                protocol: UpstreamProtocol::Http1,
                pool_wait_ms: 0,
                request_send_ms,
                flags: Vec::new(),
            },
        )?
    } else {
        let response_body = {
            let mut upstream_io = DeadlineIo::new(&mut upstream, deadline);
            read_response_body(&mut upstream_io, &head.headers)
                .map_err(|err| stage_io_error("response_body", err))?
        };
        let response_receive_ms = duration_millis(response_receive_started.elapsed());
        deadline.remaining()?;
        finish_buffered_response(
            client,
            &response_context,
            BufferedResponse {
                head,
                body: response_body.body,
                trailers: response_body.trailers,
                matched_rules: response_matched_rules,
                actions: response_actions,
                protocol: UpstreamProtocol::Http1,
                pool_wait_ms: 0,
                request_send_ms: Some(request_send_ms),
                response_receive_ms: Some(response_receive_ms),
            },
        )?
    };
    result.request_bytes = request_observation.bytes;
    result.request_body_head = request_observation.body_head;
    result.request_trailers = request_observation.trailers;
    result.flags.extend(request_observation.flags);
    Ok(result)
}

fn take_request_observation(
    request: &RawRequest,
    summary: &mut Option<RequestStreamSummary>,
) -> websocket_forward::RequestObservation {
    let Some(summary) = summary.take() else {
        return websocket_forward::RequestObservation {
            bytes: request.body.len() as u64,
            body_head: None,
            trailers: None,
            flags: Vec::new(),
        };
    };
    let mut flags = vec!["request-streamed".to_string()];
    if summary.exceeded_buffer_limit {
        flags.push("request-body-rewrite-skipped-limit".to_string());
    }
    if !summary.completed {
        flags.push("request-stream-ended-by-upstream".to_string());
    }
    websocket_forward::RequestObservation {
        bytes: summary.bytes,
        body_head: Some(summary.body_head),
        trailers: Some(summary.trailers),
        flags,
    }
}
