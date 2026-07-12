use super::query::split_query;
use super::respond_json;
use crate::app::SharedState;
use crate::http::RawRequest;
use std::io::Write;

mod ca;
mod replay;
mod rules;
mod sessions;
mod status;
mod trace;
mod values;

pub(super) fn dispatch<W: Write + ?Sized>(
    stream: &mut W,
    request: &RawRequest,
    state: &SharedState,
) -> std::io::Result<()> {
    let (path, query) = split_query(&request.target);
    match (request.method.as_str(), path) {
        ("GET", "/api/status") => status::get(stream, state),
        ("GET", "/api/rules") => rules::list(stream, state),
        ("GET", "/api/rules/export") => rules::export(stream, state),
        ("POST", "/api/rules/check") => rules::check(stream, &request.body),
        ("GET", "/api/rules/test") => rules::test(stream, state, query),
        (method, path) if path.starts_with("/api/rules/") => {
            rules::group(stream, state, method, path, &request.body)
        }
        ("GET", "/api/ca/root.pem") | ("GET", "/rsproxy.crt") => ca::root(stream, state),
        ("GET", "/api/values") => values::list(stream, state),
        ("GET", "/api/values.txt") => values::list_text(stream, state),
        ("GET", path) if path.starts_with("/api/values/") => values::get(stream, state, path),
        ("PUT" | "POST", path) if path.starts_with("/api/values/") => {
            values::set(stream, state, path, &request.body)
        }
        ("DELETE", path) if path.starts_with("/api/values/") => values::delete(stream, state, path),
        ("GET", "/api/sessions") => sessions::list(stream, state, query),
        ("GET", "/api/sessions.txt") => sessions::list_text(stream, state, query),
        ("GET", "/api/sessions/follow") => sessions::follow_live(stream, state, query),
        ("GET", "/api/sessions.ndjson") => sessions::follow(stream, state, query),
        ("GET", "/api/sessions/spill.ndjson") => sessions::spill(stream, state),
        ("GET", "/api/sessions/export.json") => sessions::export_json(stream, state),
        ("GET", "/api/sessions/export.har") => sessions::export_har(stream, state),
        ("GET", path) if path.starts_with("/api/sessions/") => sessions::get(stream, state, path),
        ("POST", path) if path.starts_with("/api/replay/") => replay::run(stream, state, path),
        ("GET", "/api/trace/stats") => trace::stats(stream, state),
        ("POST", "/api/trace/clear") => trace::clear(stream, state),
        _ => respond_json(stream, 404, "{\"error\":\"not found\"}"),
    }
}
