use super::super::http;
use super::super::query::query_get;
use super::ControlState;
use super::respond_json;
use crate::shapes;
use std::io::Write;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

const PREFIX: &str = "/api/sessions/";
const FOLLOW_CAPACITY: usize = 256;
const FOLLOW_HEARTBEAT_MS: u64 = 15_000;

pub(super) fn list<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    query: Option<&str>,
) -> std::io::Result<()> {
    let sessions = state.trace.list(limit(query, 20));
    let body = format!(
        "[{}]",
        sessions
            .iter()
            .map(shapes::session_summary)
            .collect::<Vec<_>>()
            .join(",")
    );
    respond_json(stream, 200, &body)
}

pub(super) fn list_text<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    query: Option<&str>,
) -> std::io::Result<()> {
    let sessions = state.trace.list(limit(query, 20));
    http::write_response(
        stream,
        200,
        "OK",
        &[("Content-Type".to_string(), "text/plain".to_string())],
        shapes::sessions_table(&sessions).as_bytes(),
    )
}

pub(super) fn follow<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    query: Option<&str>,
) -> std::io::Result<()> {
    let after = query_get(query, "after")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let sessions = state.trace.list_after(after, limit(query, 100));
    let body = sessions
        .iter()
        .map(shapes::session_summary)
        .collect::<Vec<_>>()
        .join("\n");
    http::write_response(
        stream,
        200,
        "OK",
        &[(
            "Content-Type".to_string(),
            "application/x-ndjson".to_string(),
        )],
        body.as_bytes(),
    )
}

pub(super) fn follow_live<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    query: Option<&str>,
) -> std::io::Result<()> {
    let after = query_get(query, "after")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    let backlog_limit = limit(query, 100).min(1000);
    let heartbeat_ms = query_get(query, "heartbeat_ms")
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(FOLLOW_HEARTBEAT_MS)
        .clamp(100, 30_000);
    let mut follow = state
        .trace
        .follow(after, backlog_limit, FOLLOW_CAPACITY)
        .ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "trace collector is unavailable",
            )
        })?;

    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;
    loop {
        match follow.recv_timeout(Duration::from_millis(heartbeat_ms)) {
            Ok(session) => {
                writeln!(stream, "{}", shapes::session_summary(session.as_ref()))?;
                stream.flush()?;
            }
            Err(RecvTimeoutError::Timeout) => {
                stream.write_all(b"\n")?;
                stream.flush()?;
            }
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        }
    }
}

pub(super) fn spill<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
) -> std::io::Result<()> {
    if state.trace.spill_path().is_none() {
        return respond_json(stream, 404, "{\"error\":\"spill not configured\"}");
    }
    let body = match state.trace.spill_ndjson() {
        Ok(body) => body,
        Err(error) => {
            return respond_json(
                stream,
                500,
                &format!("{{\"error\":{}}}", shapes::string(&error.to_string())),
            );
        }
    };
    http::write_response(
        stream,
        200,
        "OK",
        &[(
            "Content-Type".to_string(),
            "application/x-ndjson".to_string(),
        )],
        &body,
    )
}

pub(super) fn export_json<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
) -> std::io::Result<()> {
    let sessions = state.trace.list(usize::MAX);
    respond_json(stream, 200, &shapes::sessions_json(&sessions))
}

pub(super) fn export_har<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
) -> std::io::Result<()> {
    let sessions = state.trace.list(usize::MAX);
    respond_json(stream, 200, &shapes::sessions_har(&sessions))
}

pub(super) fn get<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    path: &str,
) -> std::io::Result<()> {
    let id = path
        .strip_prefix(PREFIX)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(0);
    match state.trace.get(id) {
        Some(session) => respond_json(stream, 200, &shapes::session_detail(&session)),
        None => respond_json(stream, 404, "{\"error\":\"not found\"}"),
    }
}

fn limit(query: Option<&str>, default: usize) -> usize {
    query_get(query, "limit")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}
