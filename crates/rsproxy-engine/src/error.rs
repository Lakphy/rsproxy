use crate::RuleStoreError;
use rsproxy_net::NetError;
use rsproxy_rules::RuleModelError;
use std::error::Error as StdError;
use std::io;
use thiserror::Error;

/// A proxy-engine failure.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum EngineError {
    /// A failure in the networking layer.
    #[error("{0}")]
    Net(#[from] NetError),

    /// A failure while loading, compiling, watching, or storing rules.
    #[error("{0}")]
    RuleStore(#[from] RuleStoreError),

    /// An engine-owned I/O operation failed.
    #[error("{context}: {source}")]
    Io {
        /// The operation that failed.
        context: String,
        /// The underlying I/O failure.
        #[source]
        source: io::Error,
    },

    /// Certificate parsing, key generation, or signing failed.
    #[error("certificate operation failed: {0}")]
    Certificate(#[from] rcgen::Error),

    /// A structured rule value used by an engine entry point was invalid.
    ///
    /// The display prefix intentionally matches `InvalidInput` while the
    /// source chain retains the rules crate's precise category.
    #[error("invalid input: {0}")]
    RuleModel(#[from] RuleModelError),

    /// User-provided or externally supplied input is invalid.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The requested engine capability is not supported.
    #[error("unsupported engine operation: {0}")]
    Unsupported(String),
}

/// A convenient result alias for engine operations.
pub type EngineResult<T> = Result<T, EngineError>;

// RuleStoreError predates the typed engine facade. Implementing Error here lets
// the additive EngineError wrapper retain its existing variants and signatures.
impl StdError for RuleStoreError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Watch(source) => Some(source),
            _ => None,
        }
    }
}
