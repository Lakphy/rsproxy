//! Proxy policy and data-plane runtime for rsproxy.
//!
//! The crate combines rule evaluation, protocol transports and trace capture
//! behind typed configuration and control handles. It deliberately excludes
//! CLI parsing, control-protocol routing and operating-system integration.
//!
//! [`SharedState`] is the data-plane ownership root. Clones may be sent to
//! connection threads: they share atomically published rule snapshots, the
//! trace collector, DNS resolver, and bounded caches, while configuration is
//! immutable after construction. [`EngineHandle`] is the narrower control-plane
//! view for status, rule updates, trace access, and replay.
//!
//! Every stage timeout in [`ProxyConfig`] is clipped to the remaining
//! request-total deadline. Limits are expressed in bytes or item counts at the
//! protocol boundary where they are enforced; exceeding a buffering limit keeps
//! streaming possible but skips transformations that require a complete body.

#[cfg(feature = "bench-support")]
#[doc(hidden)]
pub mod benchmark_support;
mod bounded_io;
mod certificate;
mod error;
mod handle;
mod proxy;
mod replay;
mod rule_store;
/// Runtime configuration and cloneable shared data-plane state.
pub mod state;

pub use certificate::{CaMaterial, IssuedLeafCertificate, issue_leaf_certificate};
pub use error::{EngineError, EngineResult};
pub use handle::{
    DnsStatusSnapshot, EngineConfigStatus, EngineHandle, EngineStatusSnapshot, ReplayResponse,
    UpstreamRootStatus,
};
pub use proxy::serve;
pub use rule_store::{RuleGroup, RuleSnapshot, RuleStore, RuleStoreError, RuleWatchStatus};
pub use state::{ProxyConfig, SharedState};
