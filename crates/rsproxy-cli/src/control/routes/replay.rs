use super::respond_json;
use crate::app::SharedState;
use crate::control::replay::replay_session;
use crate::json;
use std::io::Write;

const PREFIX: &str = "/api/replay/";

pub(super) fn run<W: Write + ?Sized>(
    stream: &mut W,
    state: &SharedState,
    path: &str,
) -> std::io::Result<()> {
    let id = path
        .strip_prefix(PREFIX)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    match state.trace.get(id) {
        Some(session) => match replay_session(
            &session,
            state.config.max_header_size,
            state.config.max_header_count,
        ) {
            Ok(body) => respond_json(stream, 200, &body),
            Err(error) => respond_json(
                stream,
                502,
                &format!("{{\"error\":{}}}", json::string(&error.to_string())),
            ),
        },
        None => respond_json(stream, 404, "{\"error\":\"not found\"}"),
    }
}
