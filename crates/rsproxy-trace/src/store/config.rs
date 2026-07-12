use crate::spill::TraceSpillConfig;

pub const DEFAULT_TRACE_QUEUE_CAPACITY: usize = 8192;
pub const DEFAULT_TRACE_MEMORY_BUDGET: usize = 256 * 1024 * 1024;
pub const DEFAULT_TRACE_QUEUE_MEMORY_BUDGET: usize = 64 * 1024 * 1024;
pub const DEFAULT_TRACE_BODY_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug)]
pub struct TraceStoreConfig {
    pub max_sessions: usize,
    pub queue_capacity: usize,
    pub memory_budget_bytes: usize,
    pub queue_memory_budget_bytes: Option<usize>,
    pub body_limit: usize,
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
