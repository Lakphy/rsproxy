use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use std::io;

mod body;
mod message;
mod runtime;
mod server;

pub(crate) use runtime::h2_runtime;
pub(crate) use server::serve_mitm;

type H2Body = BoxBody<Bytes, io::Error>;

#[cfg(test)]
use message::{hyper_response, raw_request};

#[cfg(test)]
#[path = "h2/tests/mod.rs"]
mod tests;
