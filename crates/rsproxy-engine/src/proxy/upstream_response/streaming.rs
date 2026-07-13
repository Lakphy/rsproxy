use super::*;

pub(in crate::proxy) fn finish_streaming_response<W, B>(
    client: &mut W,
    context: &ResponseContext<'_>,
    response: StreamingResponse<B>,
) -> io::Result<ForwardResult>
where
    W: WsIo + Send,
    B: ResponseBodyStream,
{
    let StreamingResponse {
        mut head,
        mut body,
        prefix,
        matched_rules: response_matched_rules,
        actions: response_actions,
        protocol,
        pool_wait_ms,
        request_send_ms,
        mut flags,
    } = response;
    let req = context.request;
    let meta = context.meta;
    let state = context.state;
    let trace_id = context.trace_id;
    let upstream_addr = context.upstream_addr.clone();
    let client_connection = context.client_connection;
    let deadline = context.deadline;
    let declared_trailers = declared_trailer_names(&head.headers, &response_actions);
    let mut response_headers = head.headers.clone();
    apply_streaming_response_actions(
        &mut head,
        &mut response_headers,
        meta,
        &response_actions,
        state,
    )?;
    deadline.remaining()?;
    head.version = client_response_version(&req.version).to_string();
    strip_hop_by_hop_headers(&mut response_headers);

    let body_allowed = !req.method.eq_ignore_ascii_case("HEAD")
        && !(100..200).contains(&head.status)
        && !matches!(head.status, 204 | 304);
    let content_length = http::header(&response_headers, "content-length");
    let chunked = body_allowed
        && !req.version.eq_ignore_ascii_case("HTTP/1.0")
        && (content_length.is_none() || !declared_trailers.is_empty());
    let mut response_connection = client_connection;
    if chunked {
        http::remove_header(&mut response_headers, "content-length");
        http::set_header(
            &mut response_headers,
            "Transfer-Encoding",
            "chunked".to_string(),
        );
        if !declared_trailers.is_empty() {
            http::set_header(
                &mut response_headers,
                "Trailer",
                declared_trailers.join(", "),
            );
        }
    } else if body_allowed && http::header(&response_headers, "content-length").is_none() {
        response_connection = ClientPersistence::Close;
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
            trailers: Vec::new(),
        });
    }

    http::write_response_head_with_connection(
        client,
        &head,
        &response_headers,
        response_connection.keep_alive(),
    )?;
    client.flush()?;

    flags.push("response-streamed".to_string());
    let trace_limit = trace_body_limit_for_headers(&state.config, &response_headers);
    let bytes_per_sec = throttle_bps(&response_actions, Phase::Res);
    let mut throttle = ThrottlePacer::new(bytes_per_sec);
    let mut trace = (trace_id != 0).then(|| {
        BodyTraceEmitter::new(
            &state.trace,
            trace_id,
            rsproxy_trace::BodyDirection::Response,
            trace_limit,
        )
    });
    let mut summary = StreamSummary::new(trace_limit);
    if body_allowed && !prefix.is_empty() {
        summary.observe(&prefix);
        if let Some(trace) = &mut trace {
            trace.observe_slice(&prefix);
        }
        if let Err(error) = write_stream_data(client, &prefix, chunked, &mut throttle) {
            summary.fail_downstream(error, &mut flags);
        }
    }

    while summary.error.is_none() {
        let Some(frame) = body.next_frame() else {
            break;
        };
        match frame {
            Ok(UpstreamBodyFrame::Data(data)) => {
                if !body_allowed {
                    continue;
                }
                summary.observe(&data);
                if let Some(trace) = &mut trace {
                    trace.observe_bytes(&data);
                }
                if let Err(error) = write_stream_data(client, &data, chunked, &mut throttle) {
                    summary.fail_downstream(error, &mut flags);
                }
            }
            Ok(UpstreamBodyFrame::Trailers(trailers)) => summary.trailers.extend(trailers),
            Err(error) => summary.fail_upstream(error, &mut flags),
        }
    }

    apply_response_trailer_actions(&mut summary.trailers, meta, &response_actions, state)?;
    if !body_allowed || req.version.eq_ignore_ascii_case("HTTP/1.0") {
        summary.trailers.clear();
    }
    if summary.error.is_none()
        && let Err(error) = finish_stream_body(client, chunked, &summary.trailers)
    {
        summary.fail_downstream(error, &mut flags);
    }
    if summary.error.is_some() {
        response_connection = ClientPersistence::Close;
    }
    let response_receive_ms = body.receive_ms();

    Ok(ForwardResult {
        status: head.status,
        upstream: upstream_addr,
        request_bytes: req.body.len() as u64,
        request_body_head: None,
        request_trailers: None,
        response_bytes: summary.bytes,
        res_headers: response_headers,
        res_trailers: summary.trailers,
        body_head: summary.body_head,
        frames: Vec::new(),
        kind: is_sse_response(&head.headers).then_some(SessionKind::Sse),
        response_matched_rules,
        response_actions,
        protocol,
        client_connection: response_connection,
        pool_wait_ms,
        request_send_ms: Some(request_send_ms),
        response_receive_ms,
        flags,
        error: summary.error,
    })
}

struct StreamSummary {
    bytes: u64,
    body_head: Vec<u8>,
    trailers: Vec<(String, String)>,
    trace_limit: usize,
    error: Option<String>,
}

impl StreamSummary {
    fn new(trace_limit: usize) -> Self {
        Self {
            bytes: 0,
            body_head: Vec::with_capacity(trace_limit.min(64 * 1024)),
            trailers: Vec::new(),
            trace_limit,
            error: None,
        }
    }

    fn observe(&mut self, data: &[u8]) {
        self.bytes = self.bytes.saturating_add(data.len() as u64);
        let remaining = self.trace_limit.saturating_sub(self.body_head.len());
        self.body_head.extend(data.iter().copied().take(remaining));
    }

    fn fail_upstream(&mut self, error: io::Error, flags: &mut Vec<String>) {
        if is_request_total_timeout(&error) {
            flags.push("upstream-timeout".to_string());
            flags.push("request-timeout".to_string());
            flags.push("request-total-timeout".to_string());
        } else {
            flags.push("upstream-response-body-error".to_string());
        }
        self.error = Some(error.to_string());
    }

    fn fail_downstream(&mut self, error: io::Error, flags: &mut Vec<String>) {
        flags.push("downstream-response-write-error".to_string());
        self.error = Some(format!("downstream response body: {error}"));
    }
}

fn write_stream_data<W: Write + ?Sized>(
    client: &mut W,
    data: &[u8],
    chunked: bool,
    throttle: &mut ThrottlePacer,
) -> io::Result<()> {
    if data.is_empty() {
        return Ok(());
    }
    if chunked {
        write!(client, "{:X}\r\n", data.len())?;
    }
    throttle.write(client, data)?;
    if chunked {
        client.write_all(b"\r\n")?;
    }
    Ok(())
}

fn finish_stream_body<W: Write + ?Sized>(
    client: &mut W,
    chunked: bool,
    trailers: &[(String, String)],
) -> io::Result<()> {
    if chunked {
        client.write_all(b"0\r\n")?;
        for (name, value) in trailers {
            write!(client, "{name}: {value}\r\n")?;
        }
        client.write_all(b"\r\n")?;
    }
    client.flush()
}

fn declared_trailer_names(headers: &[(String, String)], actions: &[ResolvedAction]) -> Vec<String> {
    let mut names = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("trailer"))
        .flat_map(|(_, value)| value.split(','))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    for item in actions {
        if let Action::ResTrailer(HeaderOp::Set { name, .. }) = &item.action
            && !names.iter().any(|seen| seen.eq_ignore_ascii_case(name))
        {
            names.push(name.clone());
        }
    }
    names
}
