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
