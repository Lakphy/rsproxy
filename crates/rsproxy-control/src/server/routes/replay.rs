use super::ControlState;
use super::respond_json;
use crate::shapes;
use rsproxy_engine::ReplayResponse;
use rsproxy_trace::Session;
use std::io::Write;

const PREFIX: &str = "/api/replay/";

pub(super) fn run<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    path: &str,
) -> std::io::Result<()> {
    let id = path
        .strip_prefix(PREFIX)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    match state.trace.get(id) {
        Some(session) => match state.engine.replay(&session) {
            Ok(response) => respond_json(stream, 200, &replay_json(&session, response)),
            Err(error) => respond_json(
                stream,
                502,
                &format!("{{\"error\":{}}}", shapes::string(&error.to_string())),
            ),
        },
        None => respond_json(stream, 404, "{\"error\":\"not found\"}"),
    }
}

fn replay_json(session: &Session, response: ReplayResponse) -> String {
    format!(
        "{{\"id\":{},\"url\":{},\"status\":{},\"response_bytes\":{},\"headers\":{},\"body_head\":{}}}",
        session.id,
        shapes::string(&session.url),
        response.status,
        response.response_bytes,
        shapes::headers(&response.headers),
        shapes::string(&String::from_utf8_lossy(&response.body_head))
    )
}
