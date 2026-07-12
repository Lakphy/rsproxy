use super::*;

pub(super) struct ResultPayload {
    pub headers: Vec<(String, String)>,
    pub trailers: Vec<(String, String)>,
    pub summary: BodySummary,
    pub client_connection: ClientPersistence,
    pub response_receive_ms: Option<u64>,
    pub kind: Option<SessionKind>,
    pub frames: Vec<FrameRecord>,
}

pub(super) struct BodyErrorPayload {
    pub headers: Vec<(String, String)>,
    pub summary: BodySummary,
    pub response_receive_ms: u64,
    pub kind: Option<SessionKind>,
    pub frames: Vec<FrameRecord>,
    pub error: io::Error,
}

pub(super) fn result(
    context: &FastResponseContext<'_>,
    head: &http::RawResponseHead,
    payload: ResultPayload,
) -> ForwardResult {
    let mut flags = vec!["h1-fast-path".to_string()];
    if payload.summary.streamed {
        flags.push("response-streamed".to_string());
    }
    ForwardResult {
        status: head.status,
        upstream: context.upstream.clone(),
        request_bytes: 0,
        request_body_head: None,
        request_trailers: None,
        response_bytes: payload.summary.bytes,
        res_headers: payload.headers,
        res_trailers: payload.trailers,
        body_head: payload.summary.body_head,
        frames: payload.frames,
        kind: payload.kind,
        response_matched_rules: Vec::new(),
        response_actions: Vec::new(),
        protocol: protocol(context.reused),
        client_connection: payload.client_connection,
        pool_wait_ms: 0,
        request_send_ms: Some(context.request_send_ms),
        response_receive_ms: payload.response_receive_ms,
        flags,
        error: None,
    }
}

pub(super) fn body_error_result(
    context: &FastResponseContext<'_>,
    head: &http::RawResponseHead,
    payload: BodyErrorPayload,
) -> FastResponseOutcome {
    let mut result = result(
        context,
        head,
        ResultPayload {
            headers: payload.headers,
            trailers: Vec::new(),
            summary: payload.summary,
            client_connection: ClientPersistence::Close,
            response_receive_ms: Some(payload.response_receive_ms),
            kind: payload.kind,
            frames: payload.frames,
        },
    );
    result
        .flags
        .push("upstream-response-body-error".to_string());
    result.error = Some(stage_io_error("response_body", payload.error).to_string());
    FastResponseOutcome {
        result,
        reusable: false,
    }
}

pub(super) fn emit_response(
    state: &SharedState,
    trace_id: u64,
    status: u16,
    headers: &[(String, String)],
    trailers: &[(String, String)],
) {
    if trace_id != 0 {
        state.trace.emit(rsproxy_trace::TraceEvent::Response {
            id: trace_id,
            status: Some(status),
            headers: headers.to_vec(),
            trailers: trailers.to_vec(),
        });
    }
}

pub(super) fn protocol(reused: bool) -> UpstreamProtocol {
    UpstreamProtocol::Http1Pooled {
        reused_connection: reused,
    }
}

pub(super) fn response_is_persistent(head: &http::RawResponseHead) -> bool {
    if header_contains_token(&head.headers, "connection", "close") {
        return false;
    }
    head.version.eq_ignore_ascii_case("HTTP/1.1")
        || (head.version.eq_ignore_ascii_case("HTTP/1.0")
            && header_contains_token(&head.headers, "connection", "keep-alive"))
}
