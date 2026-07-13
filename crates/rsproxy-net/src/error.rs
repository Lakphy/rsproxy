use std::fmt;
use std::io;
use thiserror::Error;

/// A network or wire-protocol failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum NetError {
    /// An I/O operation failed.
    #[error("{context}: {source}")]
    Io {
        /// The operation that failed.
        context: String,
        /// The underlying I/O failure.
        #[source]
        source: io::Error,
    },

    /// A peer sent or received data that violates a protocol contract.
    #[error("{kind} during {stage}: {message}")]
    Protocol {
        /// The class of protocol failure.
        kind: ProtocolErrorKind,
        /// The network stage where the failure occurred.
        stage: NetStage,
        /// A safe, human-readable explanation.
        message: String,
    },

    /// A network stage exceeded its time budget.
    #[error("timeout during {stage} after {timeout_ms} ms")]
    Timeout {
        /// The network stage that timed out.
        stage: NetStage,
        /// The elapsed timeout budget, in milliseconds.
        timeout_ms: u64,
    },
}

/// A convenient result alias for networking operations.
pub type NetResult<T> = Result<T, NetError>;

/// A stable category for wire-protocol failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ProtocolErrorKind {
    /// A message could not be parsed or validated.
    MalformedMessage,
    /// A valid message arrived in an invalid protocol state.
    UnexpectedMessage,
    /// Message framing was invalid or inconsistent.
    InvalidFraming,
    /// The peer selected an unsupported protocol or version.
    UnsupportedVersion,
    /// A protocol handshake failed.
    Handshake,
    /// A protocol-defined or locally configured limit was exceeded.
    LimitExceeded,
}

impl fmt::Display for ProtocolErrorKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::MalformedMessage => "malformed message",
            Self::UnexpectedMessage => "unexpected message",
            Self::InvalidFraming => "invalid framing",
            Self::UnsupportedVersion => "unsupported protocol version",
            Self::Handshake => "handshake failure",
            Self::LimitExceeded => "protocol limit exceeded",
        })
    }
}

/// The network lifecycle stage associated with an error.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum NetStage {
    /// Name resolution.
    Dns,
    /// Accepting a downstream connection.
    Accept,
    /// Establishing an upstream connection.
    Connect,
    /// Negotiating transport security.
    Tls,
    /// Waiting for capacity in an upstream connection or stream pool.
    PoolWait,
    /// Enforcing the total deadline for one request.
    RequestTotal,
    /// Waiting for the first byte of an upstream response.
    Ttfb,
    /// Sending or receiving a request.
    Request,
    /// Sending or receiving a response.
    Response,
    /// Relaying an opaque byte tunnel.
    Tunnel,
    /// Relaying WebSocket messages.
    WebSocket,
    /// Closing a connection or runtime.
    Shutdown,
}

impl fmt::Display for NetStage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Dns => "DNS resolution",
            Self::Accept => "connection accept",
            Self::Connect => "connection establishment",
            Self::Tls => "TLS handshake",
            Self::PoolWait => "upstream pool wait",
            Self::RequestTotal => "request total deadline",
            Self::Ttfb => "upstream time to first byte",
            Self::Request => "request transfer",
            Self::Response => "response transfer",
            Self::Tunnel => "tunnel relay",
            Self::WebSocket => "WebSocket relay",
            Self::Shutdown => "shutdown",
        })
    }
}
