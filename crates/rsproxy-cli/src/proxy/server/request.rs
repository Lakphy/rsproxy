use super::*;

pub(super) fn collect_connect_request<W: WsIo + Send>(
    client: &mut W,
    mut head: http::RequestHead,
    state: &SharedState,
) -> io::Result<Option<RawRequest>> {
    if !head.body.has_body() {
        return Ok(Some(head.request));
    }
    if request_expects_continue(&head.request) {
        client.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        client.flush()?;
    }
    let deadline = RequestDeadline::new(state.config.request_total_timeout)?;
    match read_request_body_bounded_with_deadline(
        client,
        head.body,
        state.config.body_buffer_limit,
        state.config.max_header_size,
        state.config.max_header_count,
        deadline,
    )? {
        http::BoundedRequestBody::Complete { body, trailers } => {
            head.request.body = body;
            head.request.trailers = trailers;
            Ok(Some(head.request))
        }
        http::BoundedRequestBody::Overflow { .. } => {
            http::write_response_with_version_and_connection(
                client,
                client_response_version(&head.request.version),
                413,
                http::reason_phrase(413),
                &[("Content-Type".to_string(), "text/plain".to_string())],
                b"CONNECT request body exceeds the configured buffer limit\n",
                false,
            )?;
            Ok(None)
        }
    }
}

pub(in crate::proxy) fn is_h1_request_input_error(error: &io::Error) -> bool {
    is_client_request_body_error(error) || is_request_total_timeout(error)
}

pub(super) fn h1_request_input_error_status(error: &io::Error) -> u16 {
    if is_request_total_timeout(error) {
        504
    } else if error.to_string().contains("limit exceeded") {
        431
    } else {
        400
    }
}

pub(in crate::proxy) fn write_h1_request_input_error<W: Write + ?Sized>(
    client: &mut W,
    request_version: &str,
    error: &io::Error,
) -> io::Result<()> {
    let status = h1_request_input_error_status(error);
    http::write_response_with_version_and_connection(
        client,
        client_response_version(request_version),
        status,
        http::reason_phrase(status),
        &[("Content-Type".to_string(), "text/plain".to_string())],
        format!("request error: {error}\n").as_bytes(),
        false,
    )
}
