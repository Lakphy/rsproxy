use super::respond_json;
use crate::app::SharedState;
use crate::json;
use std::io::Write;

pub(super) fn stats<W: Write + ?Sized>(stream: &mut W, state: &SharedState) -> std::io::Result<()> {
    respond_json(stream, 200, &json::stats(state.trace.stats()))
}

pub(super) fn clear<W: Write + ?Sized>(stream: &mut W, state: &SharedState) -> std::io::Result<()> {
    state.trace.clear();
    respond_json(stream, 200, "{\"ok\":true}")
}
