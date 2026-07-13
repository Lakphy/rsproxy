use super::ControlState;
use super::auth::{control_authorized, respond_control_unauthorized};
use super::http;
use super::routes;
use std::io::{Read, Write};

pub(super) fn handle<S: Read + Write>(mut stream: S, state: ControlState) -> std::io::Result<()> {
    let Some(request) = http::read_request(
        &mut stream,
        state.options.max_header_size,
        state.options.max_header_count,
        state.options.max_body_size,
    )?
    else {
        return Ok(());
    };
    if !control_authorized(&request.headers, state.options.api_token.as_deref()) {
        return respond_control_unauthorized(&mut stream);
    }
    routes::dispatch(&mut stream, &request, &state)
}
