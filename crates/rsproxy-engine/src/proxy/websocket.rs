use super::*;

mod concurrent;
mod nonblocking;

use concurrent::websocket_tunnel_concurrent;
use nonblocking::websocket_tunnel_nonblocking;
#[cfg(test)]
pub(in crate::proxy) use nonblocking::{flush_pending_nonblocking, read_ws_frames_nonblocking};

pub(super) struct WsFrame {
    pub(super) raw: Vec<u8>,
    pub(super) payload: Vec<u8>,
    pub(super) opcode: u8,
    pub(super) fin: bool,
}

#[derive(Default)]
pub(super) struct WsFrameDecoder {
    buf: Vec<u8>,
}

impl WsFrameDecoder {
    pub(super) fn push(&mut self, data: &[u8]) -> io::Result<Vec<WsFrame>> {
        self.buf.extend_from_slice(data);
        let mut frames = Vec::new();
        while let Some((frame, used)) = parse_ws_frame_prefix(&self.buf)? {
            self.buf.drain(..used);
            frames.push(frame);
        }
        Ok(frames)
    }
}

#[derive(Default)]
pub(super) struct WsTraceState {
    pub(super) fragmented_opcode: Option<u8>,
}

pub(super) fn is_websocket_request(headers: &[(String, String)]) -> bool {
    http::header(headers, "upgrade").is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && connection_has_token(headers, "upgrade")
}

pub(super) fn is_websocket_response(headers: &[(String, String)], status: u16) -> bool {
    status == 101
        && http::header(headers, "upgrade")
            .is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
        && connection_has_token(headers, "upgrade")
}

pub(super) fn connection_has_token(headers: &[(String, String)], token: &str) -> bool {
    http::header(headers, "connection")
        .map(|value| {
            value
                .split(',')
                .any(|part| part.trim().eq_ignore_ascii_case(token))
        })
        .unwrap_or(false)
}

pub(super) fn write_upgrade_response_head<W: Write>(
    stream: &mut W,
    head: &http::RawResponseHead,
    headers: &[(String, String)],
) -> io::Result<()> {
    let reason = if head.reason.is_empty() {
        http::reason_phrase(head.status)
    } else {
        &head.reason
    };
    write!(stream, "{} {} {}\r\n", head.version, head.status, reason)?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")
}

pub(super) fn websocket_tunnel<W: WsIo + Send>(
    client: &mut W,
    client_reader: Option<TcpStream>,
    upstream: &mut UpstreamStream,
    trace_limit: usize,
) -> io::Result<(u64, u64, Vec<FrameRecord>)> {
    if let (Some(client_reader), Some(upstream_reader)) = (client_reader, upstream.try_clone_tcp()?)
    {
        return websocket_tunnel_concurrent(
            client,
            client_reader,
            upstream,
            upstream_reader,
            trace_limit,
        );
    }
    websocket_tunnel_nonblocking(client, upstream, trace_limit)
}

pub(super) fn websocket_end_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
            | io::ErrorKind::BrokenPipe
    )
}
