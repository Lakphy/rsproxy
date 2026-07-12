use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

pub(super) struct TraceCounters {
    pub(super) next_id: AtomicU64,
    pub(super) queue_dropped: AtomicU64,
    pub(super) queue_memory_dropped: AtomicU64,
    pub(super) queue_bytes: AtomicUsize,
}

impl TraceCounters {
    pub(super) fn new() -> Self {
        Self {
            next_id: AtomicU64::new(1),
            queue_dropped: AtomicU64::new(0),
            queue_memory_dropped: AtomicU64::new(0),
            queue_bytes: AtomicUsize::new(0),
        }
    }

    pub(super) fn try_reserve(&self, bytes: usize, memory_budget_bytes: usize) -> bool {
        let mut queued = self.queue_bytes.load(Ordering::Relaxed);
        loop {
            if queued.saturating_add(bytes) > memory_budget_bytes {
                return false;
            }
            match self.queue_bytes.compare_exchange_weak(
                queued,
                queued.saturating_add(bytes),
                Ordering::AcqRel,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(actual) => queued = actual,
            }
        }
    }

    pub(super) fn release_queue_bytes(&self, bytes: usize) {
        let _ = self
            .queue_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Relaxed, |queued| {
                Some(queued.saturating_sub(bytes))
            });
    }
}
