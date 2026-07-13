//! Bounded traffic-trace assembly and storage for rsproxy.
//!
//! The crate accepts lifecycle [`TraceEvent`] values or completed [`Session`] values, assembles
//! them on a dedicated collector thread, retains a bounded in-memory history, and can persist
//! completed sessions to verified NDJSON segments. Queue, resident-memory, body-preview, and disk
//! budgets are explicit so queue and resident growth stay bounded on the proxy data path; callers
//! may also bound spill storage or deliberately disable its disk eviction.
//!
//! This crate does not observe sockets, apply rules, or expose an HTTP API. Callers decide which
//! events and payload previews are safe to record; higher layers are responsible for access
//! control and redaction before traces leave the process.

mod event;
mod model;
mod serialize;
mod spill;
mod store;

pub use event::{BodyDirection, SessionStart, TraceEvent};
pub use model::{
    FrameDataEncoding, FrameDirection, FrameRecord, Session, SessionKind, TlsRecord, now_millis,
};
pub use spill::{TraceSpillCompression, TraceSpillConfig};
pub use store::{
    DEFAULT_TRACE_BODY_LIMIT, DEFAULT_TRACE_MEMORY_BUDGET, DEFAULT_TRACE_QUEUE_CAPACITY,
    DEFAULT_TRACE_QUEUE_MEMORY_BUDGET, TraceFollow, TraceStats, TraceStore, TraceStoreConfig,
};

#[cfg(test)]
use spill::*;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
