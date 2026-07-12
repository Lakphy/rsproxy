use super::*;

mod body_stream;
mod fallback;
mod pool;
mod response;

use pool::{FastConnection, FastPermit, acquire, checkin, checkout};

pub(super) fn try_pooled<W: WsIo + Send>(
    client: &mut W,
    ctx: &ForwardCtx<'_>,
    network_timings: &mut NetworkTimings,
) -> io::Result<Option<ForwardResult>> {
    if !pool_eligible(ctx) {
        return Ok(None);
    }

    let pool_key = ctx.route.connect_addr();
    let mut connection = checkout(&pool_key);
    let mut reused = connection.is_some();
    for attempt in 0..2 {
        let mut active = match connection.take() {
            Some(connection) => connection,
            None => {
                let permit = acquire(
                    &pool_key,
                    ctx.state.config.h1_pool_max_active_per_key,
                    ctx.deadline
                        .budget(ctx.state.config.h1_pool_wait_timeout)?
                        .timeout(),
                )?;
                connect(ctx, network_timings, permit)?
            }
        };
        match send_request(&mut active, ctx, network_timings) {
            Ok((head, request_send_ms)) => {
                let mut outcome =
                    response::finish(client, ctx, head, &mut active, reused, request_send_ms)?;
                if !outcome
                    .result
                    .flags
                    .iter()
                    .any(|flag| flag == "h1-fast-path")
                {
                    outcome.result.flags.push("h1-fast-path".to_string());
                }
                if outcome.reusable {
                    checkin(&pool_key, active);
                }
                return Ok(Some(outcome.result));
            }
            Err(error) if reused && attempt == 0 => {
                reused = false;
                connection = None;
                tracing::debug!(
                    event = "h1_fast_stale_connection",
                    error = %error,
                    "retrying an idempotent request on a fresh upstream connection"
                );
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!("fast HTTP/1 retry loop returns within two attempts")
}

pub(super) fn pool_eligible(ctx: &ForwardCtx<'_>) -> bool {
    ctx.rules.rules.is_empty()
        && ctx.actions.is_empty()
        && ctx.route.is_direct()
        && ctx.url.scheme == "http"
        && matches!(ctx.request.method.as_str(), "GET" | "HEAD")
        && ctx.request.version.eq_ignore_ascii_case("HTTP/1.1")
        && ctx.request.body.is_empty()
        && ctx.request.trailers.is_empty()
        && http::header(&ctx.request.headers, "expect").is_none()
        && !ctx.websocket_request()
        && !accepts_sse(&ctx.request.headers)
}

fn connect(
    ctx: &ForwardCtx<'_>,
    network_timings: &mut NetworkTimings,
    permit: FastPermit,
) -> io::Result<FastConnection> {
    let stream = connect_tcp_with_timeouts(
        &ctx.route.connect_addr(),
        ctx.state,
        network_timings,
        ctx.deadline,
    )?;
    stream.set_nodelay(true)?;
    FastConnection::new(stream, permit)
}

fn send_request(
    connection: &mut FastConnection,
    ctx: &ForwardCtx<'_>,
    network_timings: &mut NetworkTimings,
) -> io::Result<(http::RawResponseHead, u64)> {
    let mut encoded = Vec::with_capacity(512);
    write!(
        encoded,
        "{} {} HTTP/1.1\r\n",
        ctx.request.method,
        ctx.url.origin_form()
    )?;
    for (name, value) in ctx.headers {
        if !name.eq_ignore_ascii_case("connection") {
            write!(encoded, "{name}: {value}\r\n")?;
        }
    }
    encoded.extend_from_slice(b"Connection: keep-alive\r\n\r\n");

    let write_budget = ctx.deadline.budget(UPSTREAM_WRITE_TIMEOUT)?;
    connection.set_write_timeout(write_budget.timeout())?;
    let request_started = Instant::now();
    connection
        .writer
        .write_all(&encoded)
        .map_err(|error| write_budget.map_timeout(error))?;
    let request_send_ms = duration_millis(request_started.elapsed());

    let ttfb_budget = ctx
        .deadline
        .budget(ctx.state.config.upstream_ttfb_timeout)?;
    connection.set_read_timeout(ttfb_budget.timeout())?;
    let response_started = Instant::now();
    let head = http::read_response_head_buffered(
        &mut connection.reader,
        ctx.state.config.max_header_size,
        ctx.state.config.max_header_count,
    )
    .map_err(|error| ttfb_budget.map_timeout(error))?;
    network_timings.ttfb_ms = network_timings
        .ttfb_ms
        .saturating_add(duration_millis(response_started.elapsed()));
    Ok((head, request_send_ms))
}

pub(super) use fallback::forward_unpooled;
