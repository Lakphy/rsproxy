use crate::request_deadline::{RequestDeadline, is_request_total_timeout};
use crate::runtime::h2_runtime;
use crate::transfer_timing::{TransferTimer, timed_body};
use crate::upstream_body::UpstreamBody;
use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use hyper::client::conn::http2::SendRequest;
use std::io;
use std::time::{Duration, Instant};

mod connection;
mod message;
mod pool;
mod request_body;
mod streaming;

use connection::{SendOutcome, send_on_sender};
use message::*;
use pool::*;
use request_body::H2RequestBodySender;
pub use streaming::StreamingH2Request;

type RequestBody = BoxBody<Bytes, io::Error>;
type H2Sender = SendRequest<RequestBody>;

#[derive(Clone, Debug)]
/// Buffered HTTP/2 request prepared for an upstream origin.
pub struct UpstreamH2Request {
    /// HTTP method token.
    pub method: String,
    /// Absolute or origin URI accepted by Hyper's HTTP/2 request builder.
    pub uri: String,
    /// Request headers before HTTP/2 validation and normalization.
    pub headers: Vec<(String, String)>,
    /// Buffered data payload; ignored when streaming mode is selected.
    pub body: Vec<u8>,
    /// Terminal request trailers for buffered mode.
    pub trailers: Vec<(String, String)>,
}

#[derive(Debug)]
/// Upstream HTTP/2 response plus connection and transfer timings.
pub struct UpstreamH2Response {
    /// Numeric response status.
    pub status: u16,
    /// Validated response header fields.
    pub headers: Vec<(String, String)>,
    /// Streaming response body driven by the HTTP/2 connection task.
    pub body: UpstreamBody,
    /// Whether the request used an existing pooled connection.
    pub reused_connection: bool,
    /// Milliseconds from dispatch entry until a keyed stream slot was acquired.
    pub pool_wait_ms: u64,
    /// Milliseconds spent making the request body available to the connection task.
    pub request_send_ms: u64,
    /// Milliseconds from completing request send until response headers arrived.
    pub ttfb_ms: u64,
}

enum H2Dispatch {
    Response(UpstreamH2Response),
    Connect(H2PoolLease),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Request body delivery mode selected for an HTTP/2 dispatch.
pub enum H2Body {
    /// Send the request's stored body and trailers before returning.
    Buffered,
    /// Return a handle through which the caller supplies frames incrementally.
    Streaming,
}

#[derive(Clone, Copy)]
/// Limits and clocks shared by one upstream HTTP/2 dispatch.
pub struct H2Config {
    /// Maximum encoded request or decoded response header bytes.
    pub max_header_size: usize,
    /// Maximum request or response header fields.
    pub max_header_count: usize,
    /// Maximum concurrent streams admitted for one pool key.
    pub max_active_streams_per_key: usize,
    /// Pool admission duration measured from the beginning of [`dispatch`].
    pub pool_wait_timeout: Duration,
    /// Response-head wait measured after the request has been sent.
    pub ttfb_timeout: Duration,
    /// Request-total deadline created by the caller before any network stage.
    pub deadline: RequestDeadline,
}

/// Inputs for reusing or creating an upstream HTTP/2 connection.
pub struct H2DispatchRequest<'a> {
    /// Stable key identifying connections safe to reuse together.
    pub pool_key: &'a str,
    /// Request metadata and any buffered payload.
    pub request: UpstreamH2Request,
    /// Buffered or streaming request-body mode.
    pub body: H2Body,
    /// Per-request limits, stage timeouts, and total deadline.
    pub config: H2Config,
}

/// First result of dispatching against the HTTP/2 connection pool.
pub enum H2Outcome {
    /// A buffered request completed on an existing connection.
    Response(UpstreamH2Response),
    /// A streaming request started on an existing connection.
    Streaming(StreamingH2Request),
    /// No reusable connection exists; the caller must establish a transport.
    Connect(H2Connector),
}

/// Result after attaching a newly established transport to a connector.
pub enum H2Connected {
    /// The buffered request completed on the new connection.
    Response(UpstreamH2Response),
    /// The streaming request is ready to accept body frames.
    Streaming(StreamingH2Request),
}

/// Reserved pool slot awaiting a caller-established upstream transport.
pub struct H2Connector {
    lease: H2PoolLease,
    request: UpstreamH2Request,
    body: H2Body,
    config: H2Config,
}

/// Reuses a pooled HTTP/2 connection or reserves a slot for a new one.
///
/// Pool waiting starts on entry and is clipped to `input.config.deadline`.
pub fn dispatch(input: H2DispatchRequest<'_>) -> io::Result<H2Outcome> {
    let H2DispatchRequest {
        pool_key,
        request,
        body,
        config,
    } = input;
    // Prune an expired idle sender before this request's lease increments the
    // active-stream count and makes the entry look busy.
    let _ = h2_pool()
        .inner
        .lock()
        .expect("HTTP/2 pool lock poisoned")
        .get(pool_key);
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
    /// Performs the HTTP/2 handshake and starts the reserved request on `stream`.
    ///
    /// Handshake time consumes the request-total deadline supplied to [`dispatch`].
    pub fn connect<S: crate::async_io::ReadyIo>(self, stream: S) -> io::Result<H2Connected> {
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
