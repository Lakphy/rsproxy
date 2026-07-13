use super::*;

mod handshake;
mod records;
mod timing;

pub(super) use handshake::{TlsWrapInput, tls_wrap_upstream_stream};
pub(super) use records::{client_tls_record, failed_tls_record, server_tls_record};
pub(super) use timing::{connect_tcp_with_timeouts, duration_millis, read_response_head_with_ttfb};

#[cfg(test)]
pub(super) use handshake::tls_handshake_io_error;
#[cfg(test)]
pub(super) use timing::tcp_connect_timeout_error;
