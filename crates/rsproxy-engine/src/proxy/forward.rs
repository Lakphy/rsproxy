use super::*;

mod context;
mod plan;
mod streaming_h2;

pub(in crate::proxy) use context::{ForwardCtx, ForwardInput};
use plan::{UpstreamPlan, plan_upstream};

pub(super) fn forward<W: WsIo + Send>(
    client: &mut W,
    input: ForwardInput<'_>,
    tls_records: &mut Vec<TlsRecord>,
    network_timings: &mut NetworkTimings,
) -> io::Result<ForwardResult> {
    let ForwardInput {
        request,
        full_url,
        meta,
        actions,
        state,
        trace_id,
        rules,
        plain_client_clone,
        client_connection,
        deadline,
        mut request_body,
        request_body_rules_skipped,
    } = input;
    let url =
        UrlParts::parse(full_url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    http::validate_request_trailers(
        &request.trailers,
        state.config.max_header_size,
        state.config.max_header_count,
    )
    .map_err(|err| stage_error("request_trailer", err))?;
    let route = upstream_route(&url, actions, meta, state)?;
    let mut headers = request.headers.clone();
    let websocket_request = is_websocket_request(&request.headers);
    http::remove_header(&mut headers, "expect");
    http::remove_header(&mut headers, "proxy-connection");
    http::remove_header(&mut headers, "proxy-authorization");
    if !websocket_request {
        http::remove_header(&mut headers, "connection");
    }
    http::set_header(&mut headers, "Host", host_header(&url));
    if websocket_request {
        http::set_header(&mut headers, "Connection", "Upgrade".to_string());
    }
    if let Some(body) = request_body.as_ref() {
        prepare_streaming_upstream_request_framing(&mut headers, body.framing());
    } else {
        prepare_upstream_request_framing(&mut headers, request);
    }

    let ctx = ForwardCtx {
        request,
        full_url,
        url: &url,
        meta,
        actions,
        state,
        trace_id,
        rules,
        route: &route,
        headers: &headers,
        client_connection,
        deadline,
        request_body_rules_skipped,
    };
    let streaming = request_body.is_some();
    let plan = plan_upstream(&ctx, streaming);

    if matches!(plan, UpstreamPlan::H1 { pooled: true })
        && let Some(result) = h1_forward::try_pooled(client, &ctx, network_timings)?
    {
        return Ok(result);
    }

    let h2_request = UpstreamH2Request {
        method: request.method.clone(),
        uri: full_url.to_string(),
        headers: headers.clone(),
        body: request.body.clone(),
        trailers: request.trailers.clone(),
    };
    let h2_pool_key = match plan {
        UpstreamPlan::H2 { pooled: true, .. } => {
            Some(upstream_pool_key(&url, &route, actions, meta, state)?)
        }
        _ => None,
    };
    let h2_config = H2Config {
        max_header_size: state.config.max_header_size,
        max_header_count: state.config.max_header_count,
        max_active_streams_per_key: state.config.h2_pool_max_active_streams_per_key,
        pool_wait_timeout: state.config.h2_pool_wait_timeout,
        ttfb_timeout: state.config.upstream_ttfb_timeout,
        deadline,
    };
    let h2_body = if streaming {
        H2Body::Streaming
    } else {
        H2Body::Buffered
    };
    let mut h2_connector = None;
    if let Some(pool_key) = h2_pool_key.as_deref() {
        match dispatch_upstream_h2(H2DispatchRequest {
            pool_key,
            request: h2_request,
            body: h2_body,
            config: h2_config,
        })? {
            H2Outcome::Response(response) => {
                return finish_h2_response(client, &ctx, response, network_timings);
            }
            H2Outcome::Streaming(upstream) => {
                return streaming_h2::finish(
                    client,
                    &ctx,
                    request_body
                        .take()
                        .expect("streaming request body is present"),
                    upstream,
                    network_timings,
                );
            }
            H2Outcome::Connect(connector) => h2_connector = Some(connector),
        }
    }

    let allow_origin_h2 = matches!(plan, UpstreamPlan::H2 { .. });
    let upstream = connect_upstream_stream(&ctx, allow_origin_h2, tls_records, network_timings)?;
    if upstream.negotiated_h2() {
        let connector = h2_connector.take().ok_or_else(|| {
            stage_error(
                "upstream_h2",
                "origin negotiated h2 without an eligible connector",
            )
        })?;
        return match connector.connect(upstream)? {
            H2Connected::Response(response) => {
                finish_h2_response(client, &ctx, response, network_timings)
            }
            H2Connected::Streaming(upstream) => streaming_h2::finish(
                client,
                &ctx,
                request_body
                    .take()
                    .expect("streaming request body is present"),
                upstream,
                network_timings,
            ),
        };
    }
    drop(h2_connector);

    h1_forward::forward_unpooled(
        client,
        &ctx,
        plain_client_clone,
        upstream,
        network_timings,
        request_body,
    )
}

fn finish_h2_response<W: WsIo + Send>(
    client: &mut W,
    ctx: &ForwardCtx<'_>,
    response: UpstreamH2Response,
    network_timings: &mut NetworkTimings,
) -> io::Result<ForwardResult> {
    network_timings.ttfb_ms = network_timings.ttfb_ms.saturating_add(response.ttfb_ms);
    finish_h2_upstream_response(client, ctx, response)
}
