use super::*;

const SMALL_BODY_LIMIT: usize = 64 * 1024;

mod body;
mod result;

use body::*;
use result::*;

pub(super) struct FastResponseOutcome {
    pub(super) result: ForwardResult,
    pub(super) reusable: bool,
}

struct FastResponseContext<'a> {
    request: &'a RawRequest,
    meta: &'a RequestMeta,
    state: &'a SharedState,
    trace_id: u64,
    upstream: String,
    reused: bool,
    client_connection: ClientPersistence,
    request_send_ms: u64,
    deadline: RequestDeadline,
    reusable: bool,
}

pub(super) fn finish<W: WsIo + Send>(
    client: &mut W,
    forward: &ForwardCtx<'_>,
    mut head: http::RawResponseHead,
    connection: &mut FastConnection,
    reused: bool,
    request_send_ms: u64,
) -> io::Result<FastResponseOutcome> {
    connection.set_read_timeout(forward.deadline.budget(UPSTREAM_READ_TIMEOUT)?.timeout())?;
    let reusable = response_is_persistent(&head);
    let context = FastResponseContext {
        request: forward.request,
        meta: forward.meta,
        state: forward.state,
        trace_id: forward.trace_id,
        upstream: forward.upstream_addr(),
        reused,
        client_connection: forward.client_connection,
        request_send_ms,
        deadline: forward.deadline,
        reusable,
    };
    let body_allowed = !context.request.method.eq_ignore_ascii_case("HEAD")
        && !(100..200).contains(&head.status)
        && !matches!(head.status, 204 | 304);
    if !body_allowed {
        return finish_without_body(client, &context, head);
    }
    if is_sse_response(&head.headers) {
        return finish_sse(client, &context, head, connection);
    }
    if has_chunked_transfer_encoding(&head.headers) {
        return finish_chunked(client, &context, head, connection);
    }
    if let Some(length) =
        http::header(&head.headers, "content-length").and_then(|value| value.parse::<usize>().ok())
    {
        if length <= SMALL_BODY_LIMIT && length <= context.state.config.body_buffer_limit {
            let started = Instant::now();
            let mut body = vec![0; length];
            connection.reader.read_exact(&mut body)?;
            let response_context = ResponseContext {
                request: context.request,
                meta: context.meta,
                state: context.state,
                trace_id: context.trace_id,
                upstream_addr: context.upstream.clone(),
                client_connection: context.client_connection,
                deadline: context.deadline,
            };
            let result = finish_buffered_response(
                client,
                &response_context,
                BufferedResponse {
                    head,
                    body,
                    trailers: Vec::new(),
                    matched_rules: Vec::new(),
                    actions: Vec::new(),
                    protocol: protocol(context.reused),
                    pool_wait_ms: 0,
                    request_send_ms: Some(context.request_send_ms),
                    response_receive_ms: Some(duration_millis(started.elapsed())),
                },
            )?;
            return Ok(FastResponseOutcome {
                result,
                reusable: context.reusable,
            });
        }
        return finish_fixed(client, &context, head, connection, length);
    }
    head.version = client_response_version(&context.request.version).to_string();
    finish_close_delimited(client, &context, head, connection)
}

fn finish_without_body<W: WsIo + Send>(
    client: &mut W,
    context: &FastResponseContext<'_>,
    mut head: http::RawResponseHead,
) -> io::Result<FastResponseOutcome> {
    let mut headers = head.headers.clone();
    strip_hop_by_hop_headers(&mut headers);
    head.version = client_response_version(&context.request.version).to_string();
    emit_response(context.state, context.trace_id, head.status, &headers, &[]);
    http::write_response_head_with_connection(
        client,
        &head,
        &headers,
        context.client_connection.keep_alive(),
    )?;
    client.flush()?;
    Ok(FastResponseOutcome {
        result: result(
            context,
            &head,
            ResultPayload {
                headers,
                trailers: Vec::new(),
                summary: BodySummary::empty(),
                client_connection: context.client_connection,
                response_receive_ms: Some(0),
                kind: None,
                frames: Vec::new(),
            },
        ),
        reusable: context.reusable,
    })
}

fn finish_fixed<W: WsIo + Send>(
    client: &mut W,
    context: &FastResponseContext<'_>,
    mut head: http::RawResponseHead,
    connection: &mut FastConnection,
    length: usize,
) -> io::Result<FastResponseOutcome> {
    let mut headers = head.headers.clone();
    strip_hop_by_hop_headers(&mut headers);
    http::set_header(&mut headers, "Content-Length", length.to_string());
    head.version = client_response_version(&context.request.version).to_string();
    emit_response(context.state, context.trace_id, head.status, &headers, &[]);
    http::write_response_head_with_connection(
        client,
        &head,
        &headers,
        context.client_connection.keep_alive(),
    )?;
    let started = Instant::now();
    let mut summary = BodySummary::new(trace_body_limit_for_headers(
        &context.state.config,
        &headers,
    ));
    let mut trace = body_trace(context.state, context.trace_id, summary.limit);
    if let Err(error) = relay_exact(
        &mut connection.reader,
        client,
        length,
        &mut summary,
        trace.as_mut(),
        context.deadline,
    )
    .and_then(|()| client.flush())
    {
        return Ok(body_error_result(
            context,
            &head,
            BodyErrorPayload {
                headers,
                summary,
                response_receive_ms: duration_millis(started.elapsed()),
                kind: None,
                frames: Vec::new(),
                error,
            },
        ));
    }
    Ok(FastResponseOutcome {
        result: result(
            context,
            &head,
            ResultPayload {
                headers,
                trailers: Vec::new(),
                summary,
                client_connection: context.client_connection,
                response_receive_ms: Some(duration_millis(started.elapsed())),
                kind: None,
                frames: Vec::new(),
            },
        ),
        reusable: context.reusable,
    })
}

fn finish_chunked<W: WsIo + Send>(
    client: &mut W,
    context: &FastResponseContext<'_>,
    mut head: http::RawResponseHead,
    connection: &mut FastConnection,
) -> io::Result<FastResponseOutcome> {
    let declared_trailers = http::header(&head.headers, "trailer").map(str::to_string);
    let mut headers = head.headers.clone();
    strip_hop_by_hop_headers(&mut headers);
    http::set_header(&mut headers, "Transfer-Encoding", "chunked".to_string());
    if let Some(declared) = declared_trailers {
        http::set_header(&mut headers, "Trailer", declared);
    }
    head.version = client_response_version(&context.request.version).to_string();
    emit_response(context.state, context.trace_id, head.status, &headers, &[]);
    http::write_response_head_with_connection(
        client,
        &head,
        &headers,
        context.client_connection.keep_alive(),
    )?;
    let started = Instant::now();
    let mut summary = BodySummary::new(trace_body_limit_for_headers(
        &context.state.config,
        &headers,
    ));
    let mut trace = body_trace(context.state, context.trace_id, summary.limit);
    let trailers = match relay_chunked(
        &mut connection.reader,
        client,
        &mut summary,
        trace.as_mut(),
        context.state.config.max_header_size,
        context.state.config.max_header_count,
        context.deadline,
    ) {
        Ok(trailers) => trailers,
        Err(error) => {
            return Ok(body_error_result(
                context,
                &head,
                BodyErrorPayload {
                    headers,
                    summary,
                    response_receive_ms: duration_millis(started.elapsed()),
                    kind: None,
                    frames: Vec::new(),
                    error,
                },
            ));
        }
    };
    if let Err(error) = write_chunk_end(client, &trailers).and_then(|()| client.flush()) {
        return Ok(body_error_result(
            context,
            &head,
            BodyErrorPayload {
                headers,
                summary,
                response_receive_ms: duration_millis(started.elapsed()),
                kind: None,
                frames: Vec::new(),
                error,
            },
        ));
    }
    Ok(FastResponseOutcome {
        result: result(
            context,
            &head,
            ResultPayload {
                headers,
                trailers,
                summary,
                client_connection: context.client_connection,
                response_receive_ms: Some(duration_millis(started.elapsed())),
                kind: None,
                frames: Vec::new(),
            },
        ),
        reusable: context.reusable,
    })
}

fn finish_close_delimited<W: WsIo + Send>(
    client: &mut W,
    context: &FastResponseContext<'_>,
    mut head: http::RawResponseHead,
    connection: &mut FastConnection,
) -> io::Result<FastResponseOutcome> {
    let mut headers = head.headers.clone();
    strip_hop_by_hop_headers(&mut headers);
    head.version = client_response_version(&context.request.version).to_string();
    emit_response(context.state, context.trace_id, head.status, &headers, &[]);
    http::write_response_head_with_connection(client, &head, &headers, false)?;
    let started = Instant::now();
    let mut summary = BodySummary::new(trace_body_limit_for_headers(
        &context.state.config,
        &headers,
    ));
    let mut trace = body_trace(context.state, context.trace_id, summary.limit);
    if let Err(error) = relay_to_eof(
        &mut connection.reader,
        client,
        &mut summary,
        trace.as_mut(),
        context.deadline,
    )
    .and_then(|()| client.flush())
    {
        return Ok(body_error_result(
            context,
            &head,
            BodyErrorPayload {
                headers,
                summary,
                response_receive_ms: duration_millis(started.elapsed()),
                kind: None,
                frames: Vec::new(),
                error,
            },
        ));
    }
    Ok(FastResponseOutcome {
        result: result(
            context,
            &head,
            ResultPayload {
                headers,
                trailers: Vec::new(),
                summary,
                client_connection: ClientPersistence::Close,
                response_receive_ms: Some(duration_millis(started.elapsed())),
                kind: None,
                frames: Vec::new(),
            },
        ),
        reusable: false,
    })
}

fn finish_sse<W: WsIo + Send>(
    client: &mut W,
    context: &FastResponseContext<'_>,
    mut head: http::RawResponseHead,
    connection: &mut FastConnection,
) -> io::Result<FastResponseOutcome> {
    let upstream_headers = head.headers.clone();
    let mut headers = head.headers.clone();
    strip_hop_by_hop_headers(&mut headers);
    prepare_streaming_body_headers(&mut headers);
    head.version = client_response_version(&context.request.version).to_string();
    emit_response(context.state, context.trace_id, head.status, &headers, &[]);
    http::write_response_head_with_connection(client, &head, &headers, false)?;

    let started = Instant::now();
    let trace_limit = trace_body_limit_for_headers(&context.state.config, &headers);
    let mut trace = body_trace(context.state, context.trace_id, trace_limit);
    let response = stream_sse_response(
        client,
        &mut connection.reader,
        &upstream_headers,
        trace_limit,
        None,
        |data| {
            if let Some(trace) = &mut trace {
                trace.observe_slice(data);
            }
        },
    );
    let (bytes, body_head, frames) = match response {
        Ok(response) => response,
        Err(error) => {
            return Ok(body_error_result(
                context,
                &head,
                BodyErrorPayload {
                    headers,
                    summary: BodySummary::empty(),
                    response_receive_ms: duration_millis(started.elapsed()),
                    kind: Some(SessionKind::Sse),
                    frames: Vec::new(),
                    error,
                },
            ));
        }
    };
    if let Err(error) = client.flush() {
        return Ok(body_error_result(
            context,
            &head,
            BodyErrorPayload {
                headers,
                summary: BodySummary::completed(bytes, body_head),
                response_receive_ms: duration_millis(started.elapsed()),
                kind: Some(SessionKind::Sse),
                frames,
                error,
            },
        ));
    }
    Ok(FastResponseOutcome {
        result: result(
            context,
            &head,
            ResultPayload {
                headers,
                trailers: Vec::new(),
                summary: BodySummary::completed(bytes, body_head),
                client_connection: ClientPersistence::Close,
                response_receive_ms: Some(duration_millis(started.elapsed())),
                kind: Some(SessionKind::Sse),
                frames,
            },
        ),
        reusable: false,
    })
}
