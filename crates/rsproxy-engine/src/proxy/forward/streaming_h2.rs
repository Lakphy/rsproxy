use super::*;

pub(super) fn finish<W: WsIo + Send>(
    client: &mut W,
    ctx: &ForwardCtx<'_>,
    request_body: StreamingRequestBody,
    mut upstream: StreamingH2Request,
    network_timings: &mut NetworkTimings,
) -> io::Result<ForwardResult> {
    let summary = relay_request_body_to_h2(
        client,
        &mut upstream,
        request_body,
        RequestRelayConfig {
            trace_limit: trace_body_limit_for_headers(&ctx.state.config, &ctx.request.headers),
            bytes_per_sec: None,
            max_header_size: ctx.state.config.max_header_size,
            max_header_count: ctx.state.config.max_header_count,
            deadline: ctx.deadline,
            trace: (ctx.trace_id != 0).then_some((&ctx.state.trace, ctx.trace_id)),
        },
    )?;
    let response = upstream.finish(ctx.state.config.upstream_ttfb_timeout, ctx.deadline)?;
    network_timings.ttfb_ms = network_timings.ttfb_ms.saturating_add(response.ttfb_ms);
    let response_connection = if summary.completed {
        ctx.client_connection
    } else {
        ClientPersistence::Close
    };
    let mut result =
        finish_streaming_request_h2_response(client, ctx, response, response_connection)?;
    result.request_bytes = summary.bytes;
    result.request_body_head = Some(summary.body_head);
    result.request_trailers = Some(summary.trailers);
    result.flags.push("request-streamed".to_string());
    if summary.exceeded_buffer_limit {
        result
            .flags
            .push("request-body-rewrite-skipped-limit".to_string());
    }
    if !summary.completed {
        result
            .flags
            .push("request-stream-ended-by-upstream".to_string());
    }
    Ok(result)
}
