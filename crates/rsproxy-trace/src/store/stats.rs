use super::TraceStore;
use std::sync::atomic::Ordering;

#[derive(Clone, Debug)]
pub struct TraceStats {
    pub sessions: usize,
    pub max_sessions: usize,
    pub dropped: u64,
    pub queue_dropped: u64,
    pub queue_capacity: usize,
    pub queue_bytes: usize,
    pub queue_memory_budget_bytes: usize,
    pub queue_memory_dropped: u64,
    pub evicted_sessions: u64,
    pub memory_bytes: usize,
    pub completed_memory_bytes: usize,
    pub pending_memory_bytes: usize,
    pub resident_memory_budget_bytes: usize,
    pub total_memory_bytes: usize,
    pub memory_budget_bytes: usize,
    pub next_id: u64,
    pub pending_sessions: usize,
    pub incomplete_sessions: u64,
    pub orphan_events: u64,
    pub follow_subscribers: usize,
    pub follow_dropped: u64,
    pub spilled: u64,
    pub spill_path: Option<String>,
    pub spill_dir: Option<String>,
    pub spill_bytes: u64,
    pub spill_segments: usize,
    pub spill_segment_bytes: u64,
    pub spill_disk_budget_bytes: u64,
    pub spill_compression: Option<String>,
    pub spill_evicted_segments: u64,
    pub spill_errors: u64,
    pub last_spill_error: Option<String>,
    pub spill_index_entries: u64,
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
