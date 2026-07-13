use crate::spill::TraceSpillConfig;

/// Default maximum number of queued collector commands: 8,192 entries.
pub const DEFAULT_TRACE_QUEUE_CAPACITY: usize = 8192;
/// Default combined queue and resident-memory budget: 256 MiB.
pub const DEFAULT_TRACE_MEMORY_BUDGET: usize = 256 * 1024 * 1024;
/// Maximum automatically selected queue-memory share: 64 MiB.
pub const DEFAULT_TRACE_QUEUE_MEMORY_BUDGET: usize = 64 * 1024 * 1024;
/// Default retained prefix for each request and response body: 64 KiB.
pub const DEFAULT_TRACE_BODY_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug)]
/// Bounded-resource policy used to start a [`crate::TraceStore`].
pub struct TraceStoreConfig {
    /// Maximum number of completed sessions retained in resident memory.
    pub max_sessions: usize,
    /// Maximum number of commands waiting for the collector thread.
    pub queue_capacity: usize,
    /// Combined upper budget for queued commands and resident trace state, in bytes.
    pub memory_budget_bytes: usize,
    /// Explicit queue-memory share in bytes, or `None` to allocate up to one quarter automatically.
    pub queue_memory_budget_bytes: Option<usize>,
    /// Maximum retained prefix of each request and response body, in bytes.
    pub body_limit: usize,
    /// Optional policy for persisting every completed session in verified disk segments.
    pub spill: Option<TraceSpillConfig>,
}

impl Default for TraceStoreConfig {
    fn default() -> Self {
        Self {
            max_sessions: 4096,
            queue_capacity: DEFAULT_TRACE_QUEUE_CAPACITY,
            memory_budget_bytes: DEFAULT_TRACE_MEMORY_BUDGET,
            queue_memory_budget_bytes: None,
            body_limit: DEFAULT_TRACE_BODY_LIMIT,
            spill: None,
        }
    }
}

impl TraceStoreConfig {
    pub(super) fn memory_partitions(&self) -> (usize, usize) {
        let total = self.memory_budget_bytes;
        let automatic = if total == 0 {
            0
        } else {
            (total / 4).clamp(1, DEFAULT_TRACE_QUEUE_MEMORY_BUDGET)
        };
        let queue = self
            .queue_memory_budget_bytes
            .unwrap_or(automatic)
            .min(total);
        (queue, total.saturating_sub(queue))
    }
}
