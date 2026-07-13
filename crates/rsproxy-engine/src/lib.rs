#[cfg(feature = "bench-support")]
#[doc(hidden)]
pub mod benchmark_support;
mod certificate;
mod error;
mod handle;
mod proxy;
mod replay;
mod rule_store;
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
