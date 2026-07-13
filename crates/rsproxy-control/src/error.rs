use rsproxy_engine::EngineError;
use std::io;
use thiserror::Error;

/// A control-plane client, server, or transport failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ControlError {
    /// A control transport I/O operation failed.
    #[error("{context}: {source}")]
    Io {
        /// The operation that failed.
        context: String,
        /// The underlying I/O failure.
        #[source]
        source: io::Error,
    },

    /// A control-protocol message was malformed or unexpected.
    #[error("control protocol error: {0}")]
    Protocol(String),

    /// Control authentication material was missing or invalid.
    ///
    /// Display intentionally preserves the validation text consumed by the
    /// CLI presentation boundary.
    #[error("{0}")]
    Authentication(String),

    /// The control server returned a non-successful HTTP response.
    ///
    /// Display intentionally renders only the server-provided body so callers
    /// do not have to strip a duplicated status prefix from structured output.
    #[error("{body}")]
    HttpStatus {
        /// The HTTP response status code.
        status: u16,
        /// The safe response body intended for presentation to the caller.
        body: String,
    },

    /// A control request failed local validation.
    #[error("invalid control request: {0}")]
    InvalidRequest(String),

    /// The selected transport or control operation is unavailable.
    #[error("unsupported control operation: {0}")]
    Unsupported(String),

    /// A JSON control document could not be encoded or decoded.
    #[error("{context}: {source}")]
    Json {
        /// The document or operation that failed.
        context: String,
        /// The underlying JSON failure.
        #[source]
        source: serde_json::Error,
    },

    /// Secure random token generation failed.
    #[error("{context}: {source}")]
    Random {
        /// The operation that failed.
        context: String,
        /// The underlying operating-system random source failure.
        #[source]
        source: getrandom::Error,
    },

    /// A failure reported by the proxy engine.
    #[error("{0}")]
    Engine(#[from] EngineError),
}

impl ControlError {
    pub(crate) fn io(context: impl Into<String>, source: io::Error) -> Self {
        Self::Io {
            context: context.into(),
            source,
        }
    }

    pub(crate) fn protocol(message: impl Into<String>) -> Self {
        Self::Protocol(message.into())
    }
}

/// A convenient result alias for control-plane operations.
pub type ControlResult<T> = Result<T, ControlError>;

#[cfg(test)]
#[path = "error/tests.rs"]
mod tests;
