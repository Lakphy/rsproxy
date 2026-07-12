use super::auth::{control_authorized, respond_control_unauthorized};
use super::routes;
use crate::app::SharedState;
use crate::http;
use std::io::{Read, Write};

pub(super) fn handle<S: Read + Write>(mut stream: S, state: SharedState) -> std::io::Result<()> {
    let Some(request) = http::read_request(
        &mut stream,
        state.config.max_header_size,
        state.config.max_header_count,
    )?
    else {
        return Ok(());
    };
    if !control_authorized(&request.headers, state.config.api_token.as_deref()) {
        return respond_control_unauthorized(&mut stream);
    }
    routes::dispatch(&mut stream, &request, &state)
}
