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
pub struct DownstreamH2Config {
    pub max_header_size: usize,
    pub max_header_count: usize,
}

#[derive(Debug)]
pub struct DownstreamH2Request {
    pub head: RequestHead,
    pub authority: String,
    pub body: mpsc::Receiver<io::Result<DownstreamH2RequestFrame>>,
}

#[derive(Debug)]
pub enum DownstreamH2RequestFrame {
    Data(Bytes),
    Trailers(Vec<(String, String)>),
}

#[derive(Debug)]
pub struct DownstreamH2Response {
    pub head: DownstreamH2ResponseHead,
    pub body: mpsc::Receiver<io::Result<DownstreamH2ResponseFrame>>,
}

#[derive(Debug)]
pub struct DownstreamH2ResponseHead {
    pub status: u16,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum DownstreamH2ResponseFrame {
    Data(Bytes),
    Trailers(Vec<(String, String)>),
}

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
