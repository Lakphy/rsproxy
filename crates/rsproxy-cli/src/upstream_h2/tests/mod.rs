use super::*;
use crate::proxy::UpstreamStream;
use std::time::Duration;

mod connection;
mod message;
mod pool;
mod request_body;
mod streaming;
mod timeouts;

fn request_deadline() -> RequestDeadline {
    RequestDeadline::new(Duration::from_secs(5)).unwrap()
}

fn test_config(
    max_active_streams_per_key: usize,
    pool_wait_timeout: Duration,
    ttfb_timeout: Duration,
    deadline: RequestDeadline,
) -> H2Config {
    H2Config {
        max_header_size: 4096,
        max_header_count: 32,
        max_active_streams_per_key,
        pool_wait_timeout,
        ttfb_timeout,
        deadline,
    }
}

fn dispatch_buffered(
    pool_key: &str,
    request: UpstreamH2Request,
    config: H2Config,
) -> io::Result<H2Outcome> {
    dispatch(H2DispatchRequest {
        pool_key,
        request,
        body: H2Body::Buffered,
        config,
    })
}

fn expect_connector(outcome: H2Outcome) -> H2Connector {
    match outcome {
        H2Outcome::Connect(connector) => connector,
        H2Outcome::Response(_) | H2Outcome::Streaming(_) => {
            panic!("test pool unexpectedly contained a connection")
        }
    }
}

fn expect_response(outcome: H2Outcome) -> UpstreamH2Response {
    match outcome {
        H2Outcome::Response(response) => response,
        H2Outcome::Connect(_) | H2Outcome::Streaming(_) => {
            panic!("test pool did not reuse its connection")
        }
    }
}

fn connect_response(
    connector: H2Connector,
    stream: UpstreamStream,
) -> io::Result<UpstreamH2Response> {
    match connector.connect(stream)? {
        H2Connected::Response(response) => Ok(response),
        H2Connected::Streaming(_) => panic!("buffered connector started a streaming request"),
    }
}
