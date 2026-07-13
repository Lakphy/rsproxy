use super::ControlState;
use super::respond_json;
use crate::shapes;
use std::io::Write;

pub(super) fn stats<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
) -> std::io::Result<()> {
    respond_json(stream, 200, &shapes::stats(state.trace.stats()))
}

pub(super) fn clear<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
) -> std::io::Result<()> {
    state.trace.clear();
    respond_json(stream, 200, "{\"ok\":true}")
}
