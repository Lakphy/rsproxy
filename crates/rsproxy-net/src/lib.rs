//! Transport primitives shared by the rsproxy engine.
//!
//! This crate owns bounded HTTP/1 message parsing, HTTP/2 client and server
//! adaptation, DNS resolution, connection-pool admission, and request deadline
//! accounting. Callers supply policy and execute rule actions; this crate does
//! not load application configuration, choose proxy routes, issue certificates,
//! or retain trace sessions.
//!
//! Timeout values are durations rather than background timers. Unless an item
//! says otherwise, the caller starts enforcement when it begins the named I/O
//! stage, and a [`RequestDeadline`] caps all stage-local budgets for one request.
mod async_io;
mod dns;
mod downstream_h2;
mod error;
mod http;
mod request_deadline;
mod runtime;
mod transfer_timing;
mod upstream_body;
mod upstream_h2;
mod upstream_pool;

pub use async_io::{AsyncIo, ReadyIo};
pub use dns::{DnsConfig, DnsResolver, DnsStatsSnapshot};
pub use downstream_h2::{
    DownstreamH2Config, DownstreamH2Request, DownstreamH2RequestFrame, DownstreamH2Response,
    DownstreamH2ResponseFrame, DownstreamH2ResponseHead, serve_downstream_h2,
};
pub use error::{NetError, NetResult, NetStage, ProtocolErrorKind};
#[cfg(feature = "test-support")]
type BodyAndTrailers = (Vec<u8>, Vec<(String, String)>);

/// Drains a request body reader and returns its bytes and validated trailers.
#[cfg(feature = "test-support")]
pub fn read_request_body_all<R: std::io::Read + ?Sized>(
    stream: &mut R,
    reader: RequestBodyReader,
    max_header_size: usize,
    max_header_count: usize,
) -> std::io::Result<BodyAndTrailers> {
    http::read_request_body_all(stream, reader, max_header_size, max_header_count)
}
pub use http::{
    BoundedRequestBody, RawRequest, RawResponseHead, RequestBodyFraming, RequestBodyRead,
    RequestBodyReader, RequestHead, header, read_request, read_request_body_bounded,
    read_request_head, read_request_head_tcp, read_response_head, read_response_head_buffered,
    reason_phrase, remove_header, response_can_send_content, response_has_framed_body, set_header,
    status_can_send_content, validate_request_trailers, write_response, write_response_head,
    write_response_head_with_connection, write_response_with_version_and_connection,
};
pub use request_deadline::{RequestDeadline, TimeoutBudget, is_request_total_timeout};
pub use runtime::h2_runtime;
pub use upstream_body::{BoundedBody, CollectedBody, UpstreamBody, UpstreamBodyFrame};
#[cfg(feature = "test-support")]
pub use upstream_body::{TestReceiveTimer, test_timed_upstream_body_channel};
pub use upstream_h2::{
    H2Body, H2Config, H2Connected, H2Connector, H2DispatchRequest, H2Outcome, StreamingH2Request,
    UpstreamH2Request, UpstreamH2Response, dispatch,
};
pub use upstream_pool::{ActivityStore, KeyedActivity, PoolWaitSpec, acquire_slot};
