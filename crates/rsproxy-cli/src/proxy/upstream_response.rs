use super::*;

mod buffered;
mod streaming;

pub(in crate::proxy) use buffered::finish_buffered_response;
pub(in crate::proxy) use streaming::finish_streaming_response;

const COALESCED_UPSTREAM_BODY_LIMIT: usize = 64 * 1024;

pub(in crate::proxy) struct ResponseContext<'a> {
    pub request: &'a RawRequest,
    pub meta: &'a RequestMeta,
    pub state: &'a SharedState,
    pub trace_id: u64,
    pub upstream_addr: String,
    pub client_connection: ClientPersistence,
    pub deadline: RequestDeadline,
}

impl<'a> ResponseContext<'a> {
    pub fn from_forward(ctx: &ForwardCtx<'a>) -> Self {
        Self {
            request: ctx.request,
            meta: ctx.meta,
            state: ctx.state,
            trace_id: ctx.trace_id,
            upstream_addr: ctx.upstream_addr(),
            client_connection: ctx.client_connection,
            deadline: ctx.deadline,
        }
    }
}

pub(in crate::proxy) struct BufferedResponse {
    pub head: http::RawResponseHead,
    pub body: Vec<u8>,
    pub trailers: Vec<(String, String)>,
    pub matched_rules: Vec<MatchedRule>,
    pub actions: Vec<ResolvedAction>,
    pub protocol: UpstreamProtocol,
    pub pool_wait_ms: u64,
    pub request_send_ms: Option<u64>,
    pub response_receive_ms: Option<u64>,
}

pub(in crate::proxy) trait ResponseBodyStream {
    fn next_frame(&mut self) -> Option<io::Result<UpstreamBodyFrame>>;
    fn receive_ms(&self) -> Option<u64>;
}

impl ResponseBodyStream for UpstreamBody {
    fn next_frame(&mut self) -> Option<io::Result<UpstreamBodyFrame>> {
        self.next()
    }

    fn receive_ms(&self) -> Option<u64> {
        self.receive_ms()
    }
}

pub(in crate::proxy) struct StreamingResponse<B = UpstreamBody> {
    pub head: http::RawResponseHead,
    pub body: B,
    pub prefix: Vec<u8>,
    pub matched_rules: Vec<MatchedRule>,
    pub actions: Vec<ResolvedAction>,
    pub protocol: UpstreamProtocol,
    pub pool_wait_ms: u64,
    pub request_send_ms: u64,
    pub flags: Vec<String>,
}

struct StreamedUpstreamResponse {
    status: u16,
    headers: Vec<(String, String)>,
    body: UpstreamBody,
    protocol: UpstreamProtocol,
    pool_wait_ms: u64,
    request_send_ms: u64,
    request_body_rules_skipped: bool,
}

pub(super) fn finish_h2_upstream_response<W: WsIo + Send>(
    client: &mut W,
    forward: &ForwardCtx<'_>,
    response: UpstreamH2Response,
) -> io::Result<ForwardResult> {
    finish_h2_response_with_context(
        client,
        ResponseContext::from_forward(forward),
        forward.rules,
        response,
        false,
    )
}

pub(super) fn finish_streaming_request_h2_response<W: WsIo + Send>(
    client: &mut W,
    forward: &ForwardCtx<'_>,
    response: UpstreamH2Response,
    client_connection: ClientPersistence,
) -> io::Result<ForwardResult> {
    let mut context = ResponseContext::from_forward(forward);
    context.client_connection = client_connection;
    finish_h2_response_with_context(client, context, forward.rules, response, true)
}

pub(in crate::proxy) fn finish_h2_response_with_context<W: WsIo + Send>(
    client: &mut W,
    context: ResponseContext<'_>,
    rules: &RuleSet,
    response: UpstreamH2Response,
    request_body_rules_skipped: bool,
) -> io::Result<ForwardResult> {
    finish_streamed_upstream_response(
        client,
        &context,
        rules,
        StreamedUpstreamResponse {
            status: response.status,
            headers: response.headers,
            body: response.body,
            protocol: UpstreamProtocol::Http2 {
                reused_connection: response.reused_connection,
            },
            pool_wait_ms: response.pool_wait_ms,
            request_send_ms: response.request_send_ms,
            request_body_rules_skipped,
        },
    )
}

fn finish_streamed_upstream_response<W: WsIo + Send>(
    client: &mut W,
    context: &ResponseContext<'_>,
    rules: &RuleSet,
    mut response: StreamedUpstreamResponse,
) -> io::Result<ForwardResult> {
    let head = http::RawResponseHead {
        version: "HTTP/1.1".to_string(),
        status: response.status,
        reason: http::reason_phrase(response.status).to_string(),
        headers: response.headers,
    };
    let res_meta = ResponseMeta {
        status: head.status,
        headers: head.headers.clone(),
    };
    let resolved = if response.request_body_rules_skipped {
        rules.resolve_response_without_request_body(context.meta, &res_meta)
    } else {
        rules.resolve_response(context.meta, &res_meta)
    };

    let body_actions = response_actions_require_body(&resolved.actions);
    let fixed_body_length = http::header(&head.headers, "content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|length| *length <= COALESCED_UPSTREAM_BODY_LIMIT)
        .filter(|length| *length <= context.state.config.body_buffer_limit);
    let fixed_body_can_coalesce = fixed_body_length.is_some()
        && http::header(&head.headers, "trailer").is_none()
        && !context.request.method.eq_ignore_ascii_case("HEAD")
        && !(100..200).contains(&head.status)
        && !matches!(head.status, 204 | 304);
    if body_actions
        || context.request.version.eq_ignore_ascii_case("HTTP/1.0")
        || fixed_body_can_coalesce
    {
        match response
            .body
            .collect_bounded(context.state.config.body_buffer_limit)?
        {
            BoundedBody::Complete(collected) => {
                let response_receive_ms = response.body.receive_ms();
                return finish_buffered_response(
                    client,
                    context,
                    BufferedResponse {
                        head,
                        body: collected.body,
                        trailers: collected.trailers,
                        matched_rules: resolved.matched_rules,
                        actions: resolved.actions,
                        protocol: response.protocol,
                        pool_wait_ms: response.pool_wait_ms,
                        request_send_ms: Some(response.request_send_ms),
                        response_receive_ms,
                    },
                );
            }
            BoundedBody::Overflow { prefix } => {
                return finish_streaming_response(
                    client,
                    context,
                    StreamingResponse {
                        head,
                        body: response.body,
                        prefix,
                        matched_rules: resolved.matched_rules,
                        actions: resolved.actions,
                        protocol: response.protocol,
                        pool_wait_ms: response.pool_wait_ms,
                        request_send_ms: response.request_send_ms,
                        flags: vec![if body_actions {
                            "body-rewrite-skipped-limit".to_string()
                        } else {
                            "http10-body-buffer-limit".to_string()
                        }],
                    },
                );
            }
        }
    }

    finish_streaming_response(
        client,
        context,
        StreamingResponse {
            head,
            body: response.body,
            prefix: Vec::new(),
            matched_rules: resolved.matched_rules,
            actions: resolved.actions,
            protocol: response.protocol,
            pool_wait_ms: response.pool_wait_ms,
            request_send_ms: response.request_send_ms,
            flags: Vec::new(),
        },
    )
}

pub(super) fn upstream_http_version(client_version: &str) -> &str {
    if client_version.eq_ignore_ascii_case("HTTP/2")
        || client_version.eq_ignore_ascii_case("HTTP/2.0")
    {
        "HTTP/1.1"
    } else {
        client_version
    }
}
