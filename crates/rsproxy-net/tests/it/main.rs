//! Networking integration tests sharing one test binary.
#![allow(clippy::unwrap_used)]

mod dns;
mod errors;
mod http_buffered_head;
mod http_tcp_head;
mod request_deadline;
