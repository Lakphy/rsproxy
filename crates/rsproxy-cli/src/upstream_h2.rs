use crate::h2::h2_runtime;
use crate::request_deadline::{RequestDeadline, is_request_total_timeout};
use crate::transfer_timing::{TransferTimer, timed_body};
use crate::upstream_body::UpstreamBody;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::client::conn::http2::SendRequest;
use std::io;
use std::time::{Duration, Instant};

mod connection;
#[path = "upstream_message.rs"]
mod message;
mod pool;
mod request_body;
mod streaming;

use connection::{SendOutcome, send_on_sender};
use message::*;
use pool::*;
use request_body::H2RequestBodySender;
pub(crate) use streaming::StreamingH2Request;

type RequestBody = BoxBody<Bytes, io::Error>;
type H2Sender = SendRequest<RequestBody>;

#[derive(Clone, Debug)]
pub(crate) struct UpstreamH2Request {
    pub method: String,
    pub uri: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub trailers: Vec<(String, String)>,
}

#[derive(Debug)]
pub(crate) struct UpstreamH2Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: UpstreamBody,
    pub reused_connection: bool,
    pub pool_wait_ms: u64,
    pub request_send_ms: u64,
    pub ttfb_ms: u64,
}

enum H2Dispatch {
    Response(UpstreamH2Response),
    Connect(H2PoolLease),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum H2Body {
    Buffered,
    Streaming,
}

#[derive(Clone, Copy)]
pub(crate) struct H2Config {
    pub max_header_size: usize,
    pub max_header_count: usize,
    pub max_active_streams_per_key: usize,
    pub pool_wait_timeout: Duration,
    pub ttfb_timeout: Duration,
    pub deadline: RequestDeadline,
}

pub(crate) struct H2DispatchRequest<'a> {
    pub pool_key: &'a str,
    pub request: UpstreamH2Request,
    pub body: H2Body,
    pub config: H2Config,
}

pub(crate) enum H2Outcome {
    Response(UpstreamH2Response),
    Streaming(StreamingH2Request),
    Connect(H2Connector),
}

pub(crate) enum H2Connected {
    Response(UpstreamH2Response),
    Streaming(StreamingH2Request),
}

pub(crate) struct H2Connector {
    lease: H2PoolLease,
    request: UpstreamH2Request,
    body: H2Body,
    config: H2Config,
}

pub(crate) fn dispatch(input: H2DispatchRequest<'_>) -> io::Result<H2Outcome> {
    let H2DispatchRequest {
        pool_key,
        request,
        body,
        config,
    } = input;
    // Prune an expired idle sender before this request's lease increments the
    // active-stream count and makes the entry look busy.
    let _ = h2_pool().inner.lock().unwrap().get(pool_key);
    let lease = match body {
        H2Body::Buffered => match dispatch_buffered(pool_key, request.clone(), config)? {
            H2Dispatch::Response(response) => return Ok(H2Outcome::Response(response)),
            H2Dispatch::Connect(lease) => lease,
        },
        H2Body::Streaming => {
            match streaming::dispatch_streaming(pool_key, request.clone(), config)? {
                streaming::H2StreamingDispatch::Request(request) => {
                    return Ok(H2Outcome::Streaming(request));
                }
                streaming::H2StreamingDispatch::Connect(lease) => lease,
            }
        }
    };
    Ok(H2Outcome::Connect(H2Connector {
        lease,
        request,
        body,
        config,
    }))
}

impl H2Connector {
    pub(crate) fn connect(self, stream: crate::proxy::UpstreamStream) -> io::Result<H2Connected> {
        match self.body {
            H2Body::Buffered => {
                connection::connect_buffered(self.lease, stream, self.request, self.config)
                    .map(H2Connected::Response)
            }
            H2Body::Streaming => {
                streaming::connect_streaming(self.lease, stream, self.request, self.config)
                    .map(H2Connected::Streaming)
            }
        }
    }
}

fn dispatch_buffered(
    pool_key: &str,
    request: UpstreamH2Request,
    config: H2Config,
) -> io::Result<H2Dispatch> {
    let started = Instant::now();
    let pool_budget = config.deadline.budget(config.pool_wait_timeout)?;
    let mut lease = Some(
        acquire_lease(
            pool_key,
            config.max_active_streams_per_key,
            pool_budget.timeout(),
            started,
        )
        .map_err(|error| pool_budget.map_timeout(error))?,
    );
    loop {
        let Some(entry) = wait_for_entry_or_connector(
            pool_key,
            lease.as_mut().expect("HTTP/2 pool lease is present"),
            config.max_active_streams_per_key,
            pool_budget.timeout(),
            started,
        )
        .map_err(|error| pool_budget.map_timeout(error))?
        else {
            return Ok(H2Dispatch::Connect(
                lease.take().expect("HTTP/2 pool lease is present"),
            ));
        };
        let outcome = h2_runtime()?.block_on(send_on_sender(
            entry.sender,
            request.clone(),
            connection::SendContext {
                config,
                reused_connection: true,
                pool_wait_started: Some(started),
                pool_entry: Some((pool_key.to_string(), entry.generation)),
                lease: lease.take().expect("HTTP/2 pool lease is present"),
            },
        ));
        match outcome {
            SendOutcome::Response(response) => return Ok(H2Dispatch::Response(response)),
            SendOutcome::Stale(returned_lease) => {
                lease = Some(returned_lease);
                remove_pool_entry(pool_key, entry.generation);
            }
            SendOutcome::Error(error) => {
                if !is_ttfb_timeout(&error) && !is_request_total_timeout(&error) {
                    remove_pool_entry(pool_key, entry.generation);
                }
                return Err(error);
            }
        }
    }
}

fn stage_error(stage: &str, error: impl std::fmt::Display) -> io::Error {
    io::Error::other(format!("upstream_h2 {stage}: {error}"))
}

fn ttfb_timeout_error(timeout: Duration) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "upstream_h2 ttfb: timeout after {}ms",
            duration_millis(timeout)
        ),
    )
}

fn is_ttfb_timeout(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::TimedOut
        && error
            .to_string()
            .starts_with("upstream_h2 ttfb: timeout after ")
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

#[cfg(test)]
#[path = "upstream_h2/tests/mod.rs"]
mod tests;
