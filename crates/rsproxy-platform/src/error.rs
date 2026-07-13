use std::io;
use thiserror::Error;

/// An operating-system integration failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PlatformError {
    /// A platform I/O operation failed.
    #[error("{context}: {source}")]
    Io {
        /// The operation that failed.
        context: String,
        /// The underlying I/O failure.
        #[source]
        source: io::Error,
    },

    /// Persistent or operating-system state is internally inconsistent.
    #[error("invalid platform state: {0}")]
    InvalidState(String),

    /// The current operating system does not support the requested operation.
    #[error("unsupported platform operation: {0}")]
    Unsupported(String),

    /// Certificate parsing or generation failed.
    #[error("certificate operation failed: {0}")]
    Certificate(#[from] rcgen::Error),

    /// An external platform command returned an unsuccessful status.
    #[error("command `{command}` failed with status {status:?}: {output}")]
    CommandFailed {
        /// The executable or safe command label that failed.
        command: String,
        /// The process exit code, or `None` when it exited by signal.
        status: Option<i32>,
        /// Sanitized command output that is safe to render and log.
        output: String,
    },

    /// A platform operation exceeded its time budget.
    #[error("{operation} timed out after {timeout_ms} ms: {output}")]
    Timeout {
        /// The operation that timed out.
        operation: String,
        /// The elapsed timeout budget, in milliseconds.
        timeout_ms: u64,
        /// Sanitized partial output that is safe to render and log.
        output: String,
    },
}

/// A convenient result alias for platform operations.
pub type PlatformResult<T> = Result<T, PlatformError>;
