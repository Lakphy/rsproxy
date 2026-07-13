use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use std::future::Future;
use std::io;
use tokio::sync::mpsc;

use crate::{ReadyIo, RequestHead};

mod body;
mod message;
mod server;

type DownstreamH2Body = BoxBody<Bytes, io::Error>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Limits applied while translating downstream HTTP/2 messages.
pub struct DownstreamH2Config {
    /// Maximum aggregate decoded header bytes accepted per message.
    pub max_header_size: usize,
    /// Maximum decoded header fields accepted per message.
    pub max_header_count: usize,
}

#[derive(Debug)]
/// One downstream HTTP/2 request with a streaming body channel.
pub struct DownstreamH2Request {
    /// Normalized request line, headers, and body framing metadata.
    pub head: RequestHead,
    /// Validated request authority, falling back to the connection authority.
    pub authority: String,
    /// Ordered body data and trailer frames produced by the HTTP/2 driver.
    pub body: mpsc::Receiver<io::Result<DownstreamH2RequestFrame>>,
}

#[derive(Debug)]
/// A decoded downstream HTTP/2 request-body frame.
pub enum DownstreamH2RequestFrame {
    /// A non-empty body data fragment.
    Data(Bytes),
    /// The terminal validated trailer fields.
    Trailers(Vec<(String, String)>),
}

#[derive(Debug)]
/// Response returned by a downstream HTTP/2 request handler.
pub struct DownstreamH2Response {
    /// Status and headers sent before body frames.
    pub head: DownstreamH2ResponseHead,
    /// Ordered body data and trailer frames sent to the downstream peer.
    pub body: mpsc::Receiver<io::Result<DownstreamH2ResponseFrame>>,
}

#[derive(Debug)]
/// HTTP/2 response metadata emitted before the streaming body.
pub struct DownstreamH2ResponseHead {
    /// Numeric HTTP status code.
    pub status: u16,
    /// Response headers after hop-by-hop fields have been removed by the caller.
    pub headers: Vec<(String, String)>,
}

#[derive(Debug)]
/// A downstream HTTP/2 response-body frame.
pub enum DownstreamH2ResponseFrame {
    /// A non-empty body data fragment.
    Data(Bytes),
    /// The terminal response trailer fields.
    Trailers(Vec<(String, String)>),
}

/// Runs one HTTP/2 server connection until the peer closes it or a protocol error occurs.
pub fn serve_downstream_h2<S, H, F>(
    io: S,
    default_authority: String,
    config: DownstreamH2Config,
    handler: H,
) -> io::Result<()>
where
    S: ReadyIo,
    H: Fn(DownstreamH2Request) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = io::Result<DownstreamH2Response>> + Send + 'static,
{
    server::serve(io, default_authority, config, handler)
}

#[cfg(test)]
use message::{hyper_response, raw_request};

#[cfg(test)]
#[path = "downstream_h2/tests/mod.rs"]
mod tests;
