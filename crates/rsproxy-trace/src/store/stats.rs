use super::TraceStore;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug)]
/// Point-in-time resource, loss, and spill-integrity counters for a trace store.
pub struct TraceStats {
    /// Completed sessions currently retained in resident memory.
    pub sessions: usize,
    /// Configured maximum resident session count.
    pub max_sessions: usize,
    /// Total submissions or sessions lost to bounded resources; currently mirrors queue drops.
    pub dropped: u64,
    /// Commands rejected because the collector queue was full, disconnected, or over budget.
    pub queue_dropped: u64,
    /// Configured maximum command count in the collector queue.
    pub queue_capacity: usize,
    /// Estimated bytes currently reserved by queued commands.
    pub queue_bytes: usize,
    /// Maximum bytes available to queued commands.
    pub queue_memory_budget_bytes: usize,
    /// Commands rejected specifically because their memory reservation exceeded the queue budget.
    pub queue_memory_dropped: u64,
    /// Completed sessions evicted from resident memory by count or memory limits.
    pub evicted_sessions: u64,
    /// Estimated resident bytes used by completed and pending sessions.
    pub memory_bytes: usize,
    /// Estimated resident bytes used by completed sessions.
    pub completed_memory_bytes: usize,
    /// Estimated resident bytes used by incremental sessions not yet completed.
    pub pending_memory_bytes: usize,
    /// Maximum bytes available to completed and pending resident sessions.
    pub resident_memory_budget_bytes: usize,
    /// Estimated bytes used by both queued commands and resident sessions.
    pub total_memory_bytes: usize,
    /// Configured combined queue and resident-memory budget in bytes.
    pub memory_budget_bytes: usize,
    /// Identifier that will be assigned to the next new session.
    pub next_id: u64,
    /// Incremental sessions waiting for an end or abort event.
    pub pending_sessions: usize,
    /// Pending sessions discarded because resident memory could not accommodate them.
    pub incomplete_sessions: u64,
    /// Events ignored because their session identifier had no pending state.
    pub orphan_events: u64,
    /// Follow streams whose liveness tokens are still registered.
    pub follow_subscribers: usize,
    /// Completed sessions not delivered because a subscriber's live channel was full.
    pub follow_dropped: u64,
    /// Sessions successfully appended to spill storage since the last clear.
    pub spilled: u64,
    /// Active or next spill data path rendered for diagnostics.
    pub spill_path: Option<String>,
    /// Configured spill directory rendered for diagnostics.
    pub spill_dir: Option<String>,
    /// Combined on-disk bytes used by spill data and index files.
    pub spill_bytes: u64,
    /// Number of indexed spill data segments currently retained.
    pub spill_segments: usize,
    /// Preferred maximum encoded bytes per spill data segment.
    pub spill_segment_bytes: u64,
    /// Configured target disk budget for spill data and indexes, in bytes.
    pub spill_disk_budget_bytes: u64,
    /// Stable name of the configured spill compression, when spilling is enabled.
    pub spill_compression: Option<String>,
    /// Oldest spill segments removed to enforce the disk budget.
    pub spill_evicted_segments: u64,
    /// Spill initialization, append, or deletion operations that failed since the last clear.
    pub spill_errors: u64,
    /// Most recent spill failure rendered for diagnostics.
    pub last_spill_error: Option<String>,
    /// Records described by currently retained spill index files.
    pub spill_index_entries: u64,
    /// Records omitted during verified reads because data or index validation failed.
    pub spill_corrupt_records: u64,
}

impl TraceStore {
    pub(crate) fn empty_stats(&self) -> TraceStats {
        let queue_dropped = self.handle.counters.queue_dropped.load(Ordering::Relaxed);
        let queue_bytes = self.handle.counters.queue_bytes.load(Ordering::Relaxed);
        TraceStats {
            sessions: 0,
            max_sessions: self.handle.max_sessions,
            dropped: queue_dropped,
            queue_dropped,
            queue_capacity: self.handle.queue_capacity,
            queue_bytes,
            queue_memory_budget_bytes: self.handle.queue_memory_budget_bytes,
            queue_memory_dropped: self
                .handle
                .counters
                .queue_memory_dropped
                .load(Ordering::Relaxed),
            evicted_sessions: 0,
            memory_bytes: 0,
            completed_memory_bytes: 0,
            pending_memory_bytes: 0,
            resident_memory_budget_bytes: self.handle.resident_memory_budget_bytes,
            total_memory_bytes: queue_bytes,
            memory_budget_bytes: self.handle.memory_budget_bytes,
            next_id: self.handle.counters.next_id.load(Ordering::Relaxed),
            pending_sessions: 0,
            incomplete_sessions: 0,
            orphan_events: 0,
            follow_subscribers: 0,
            follow_dropped: 0,
            spilled: 0,
            spill_path: None,
            spill_dir: None,
            spill_bytes: 0,
            spill_segments: 0,
            spill_segment_bytes: 0,
            spill_disk_budget_bytes: 0,
            spill_compression: None,
            spill_evicted_segments: 0,
            spill_errors: 1,
            last_spill_error: Some("trace collector is unavailable".to_string()),
            spill_index_entries: 0,
            spill_corrupt_records: 0,
        }
    }
}
