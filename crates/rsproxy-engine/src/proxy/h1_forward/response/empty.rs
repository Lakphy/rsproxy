use super::*;

pub(super) fn finish_reset_content<W: WsIo + Send>(
    client: &mut W,
    context: &FastResponseContext<'_>,
    mut head: http::RawResponseHead,
    connection: &mut FastConnection,
) -> io::Result<FastResponseOutcome> {
    let started = Instant::now();
    let mut discarded_content = false;
    let mut dropped_forbidden_trailer = false;
    if has_chunked_transfer_encoding(&head.headers) {
        let mut summary = BodySummary::new(0);
        let mut trailers = relay_chunked(
            &mut connection.reader,
            &mut io::sink(),
            &mut summary,
            None,
            context.state.config.max_header_size,
            context.state.config.max_header_count,
            context.deadline,
        )?;
        discarded_content = summary.bytes != 0;
        dropped_forbidden_trailer = sanitize_upstream_trailers(&mut trailers, &head.headers);
    } else if let Some(length) =
        http::header(&head.headers, "content-length").and_then(|value| value.parse::<usize>().ok())
    {
        let mut summary = BodySummary::new(0);
        relay_exact(
            &mut connection.reader,
            &mut io::sink(),
            length,
            &mut summary,
            None,
            context.deadline,
        )?;
        discarded_content = summary.bytes != 0;
    }

    let mut headers = head.headers.clone();
    prepare_streaming_body_headers(&mut headers);
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

    let mut result = result(
        context,
        &head,
        ResultPayload {
            headers,
            trailers: Vec::new(),
            summary: BodySummary::empty(),
            client_connection: context.client_connection,
            response_receive_ms: Some(duration_millis(started.elapsed())),
            kind: None,
            frames: Vec::new(),
        },
    );
    if discarded_content {
        result
            .flags
            .push("upstream-205-content-discarded".to_string());
    }
    if dropped_forbidden_trailer {
        result
            .flags
            .push("forbidden-upstream-trailer-dropped".to_string());
    }
    Ok(FastResponseOutcome {
        result,
        reusable: context.reusable,
    })
}

pub(super) fn finish_without_body<W: WsIo + Send>(
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
