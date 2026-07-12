use super::memory::MemoryStore;
use super::pending::PendingSessions;
use super::{TraceCounters, TraceStats};
use crate::event::TraceEvent;
use crate::model::Session;
use crate::spill::{
    SpillReadSnapshot, TraceSpillConfig, TraceSpillState, append_spill, clear_spill,
    ensure_spill_initialized, spill_read_snapshot,
};
use std::io;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Weak};

pub(super) enum Command {
    Event {
        event: TraceEvent,
        queued_bytes: usize,
    },
    Events {
        events: Vec<TraceEvent>,
        queued_bytes: usize,
    },
    List {
        limit: usize,
        reply: Sender<Vec<Session>>,
    },
    ListAfter {
        after: u64,
        limit: usize,
        reply: Sender<Vec<Session>>,
    },
    Get {
        id: u64,
        reply: Sender<Option<Session>>,
    },
    Clear(Sender<()>),
    Stats(Sender<TraceStats>),
    SpillPath(Sender<Option<PathBuf>>),
    SpillPaths(Sender<Vec<PathBuf>>),
    SpillSnapshot(Sender<io::Result<(u64, SpillReadSnapshot)>>),
    SpillReadReport {
        generation: u64,
        corrupt: u64,
        reply: Sender<()>,
    },
    Follow {
        after: u64,
        backlog_limit: usize,
        sender: SyncSender<Arc<Session>>,
        liveness: Weak<()>,
        reply: Sender<Vec<Arc<Session>>>,
    },
    #[cfg(test)]
    Block {
        started: Sender<()>,
        release: Receiver<()>,
    },
    Shutdown(Sender<()>),
}

pub(super) struct TraceWorker {
    memory: MemoryStore,
    pending: PendingSessions,
    total_memory_budget_bytes: usize,
    queue_memory_budget_bytes: usize,
    spill: Option<TraceSpillState>,
    spilled: u64,
    spill_errors: u64,
    last_spill_error: Option<String>,
    spill_corrupt_records: u64,
    spill_read_generation: u64,
    followers: Vec<Follower>,
    follow_dropped: u64,
}

struct Follower {
    sender: SyncSender<Arc<Session>>,
    liveness: Weak<()>,
}

impl TraceWorker {
    pub(super) fn new(
        max_sessions: usize,
        total_memory_budget_bytes: usize,
        resident_memory_budget_bytes: usize,
        queue_memory_budget_bytes: usize,
        body_limit: usize,
        spill_config: Option<TraceSpillConfig>,
    ) -> Self {
        Self {
            memory: MemoryStore::new(max_sessions, resident_memory_budget_bytes),
            pending: PendingSessions::new(body_limit),
            total_memory_budget_bytes,
            queue_memory_budget_bytes,
            spill: spill_config.map(TraceSpillState::new),
            spilled: 0,
            spill_errors: 0,
            last_spill_error: None,
            spill_corrupt_records: 0,
            spill_read_generation: 0,
            followers: Vec::new(),
            follow_dropped: 0,
        }
    }

    pub(super) fn run(
        mut self,
        receiver: Receiver<Command>,
        counters: Arc<TraceCounters>,
        queue_capacity: usize,
    ) {
        while let Ok(command) = receiver.recv() {
            match command {
                Command::Event {
                    event,
                    queued_bytes,
                } => {
                    self.apply_event(event, true);
                    counters.release_queue_bytes(queued_bytes);
                }
                Command::Events {
                    events,
                    queued_bytes,
                } => {
                    for event in events {
                        self.apply_event(event, false);
                    }
                    self.enforce_memory_budget(None);
                    counters.release_queue_bytes(queued_bytes);
                }
                Command::List { limit, reply } => {
                    let _ = reply.send(self.memory.list(limit));
                }
                Command::ListAfter {
                    after,
                    limit,
                    reply,
                } => {
                    let _ = reply.send(self.memory.list_after(after, limit));
                }
                Command::Get { id, reply } => {
                    let _ = reply.send(self.memory.get(id));
                }
                Command::Clear(reply) => {
                    self.clear();
                    let _ = reply.send(());
                }
                Command::Stats(reply) => {
                    let _ = reply.send(self.stats(&counters, queue_capacity));
                }
                Command::SpillPath(reply) => {
                    let _ = reply.send(self.spill_path());
                }
                Command::SpillPaths(reply) => {
                    let _ = reply.send(self.spill_paths());
                }
                Command::SpillSnapshot(reply) => {
                    let snapshot = spill_read_snapshot(self.spill.as_mut())
                        .map(|snapshot| (self.spill_read_generation, snapshot));
                    let _ = reply.send(snapshot);
                }
                Command::SpillReadReport {
                    generation,
                    corrupt,
                    reply,
                } => {
                    if generation == self.spill_read_generation {
                        self.spill_corrupt_records = corrupt;
                        if corrupt > 0 {
                            self.last_spill_error =
                                Some(format!("skipped {corrupt} corrupt spill record(s)"));
                        }
                    }
                    let _ = reply.send(());
                }
                Command::Follow {
                    after,
                    backlog_limit,
                    sender,
                    liveness,
                    reply,
                } => self.follow(after, backlog_limit, sender, liveness, reply),
                #[cfg(test)]
                Command::Block { started, release } => {
                    let _ = started.send(());
                    let _ = release.recv();
                }
                Command::Shutdown(reply) => {
                    let _ = reply.send(());
                    break;
                }
            }
        }
    }

    fn apply_event(&mut self, event: TraceEvent, enforce_budget: bool) {
        let id = event.session_id();
        if let Some(session) = self.pending.apply(event) {
            self.commit(session);
        }
        if enforce_budget {
            self.enforce_memory_budget(Some(id));
        }
    }

    fn commit(&mut self, session: Session) {
        if let Some(spill) = self.spill.as_mut() {
            match append_spill(spill, &session) {
                Ok(()) => {
                    self.spilled = self.spilled.saturating_add(1);
                    self.last_spill_error = None;
                }
                Err(error) => self.record_spill_error(error),
            }
        }
        let session = Arc::new(session);
        self.memory.insert(Arc::clone(&session));
        if !self.followers.is_empty() {
            self.publish(session);
        }
    }

    fn clear(&mut self) {
        self.memory.clear();
        self.pending.clear();
        self.spilled = 0;
        self.spill_errors = 0;
        self.last_spill_error = None;
        self.spill_corrupt_records = 0;
        self.spill_read_generation = self.spill_read_generation.wrapping_add(1);
        if let Some(spill) = self.spill.as_mut()
            && let Err(error) = clear_spill(spill)
        {
            self.record_spill_error(error);
        }
    }

    fn stats(&mut self, counters: &TraceCounters, queue_capacity: usize) -> TraceStats {
        self.ensure_spill();
        self.prune_followers();
        let spill = self.spill.as_ref();
        let queue_dropped = counters.queue_dropped.load(Ordering::Relaxed);
        let completed_memory_bytes = self.memory.memory_bytes();
        let pending_memory_bytes = self.pending.memory_bytes();
        let queue_bytes = counters.queue_bytes.load(Ordering::Relaxed);
        let memory_bytes = completed_memory_bytes.saturating_add(pending_memory_bytes);
        TraceStats {
            sessions: self.memory.len(),
            max_sessions: self.memory.max_sessions(),
            dropped: queue_dropped,
            queue_dropped,
            queue_capacity,
            queue_bytes,
            queue_memory_budget_bytes: self.queue_memory_budget_bytes,
            queue_memory_dropped: counters.queue_memory_dropped.load(Ordering::Relaxed),
            evicted_sessions: self.memory.evicted_sessions(),
            memory_bytes,
            completed_memory_bytes,
            pending_memory_bytes,
            resident_memory_budget_bytes: self.memory.memory_budget_bytes(),
            total_memory_bytes: memory_bytes.saturating_add(queue_bytes),
            memory_budget_bytes: self.total_memory_budget_bytes,
            next_id: counters.next_id.load(Ordering::Relaxed),
            pending_sessions: self.pending.len(),
            incomplete_sessions: self.pending.incomplete_sessions(),
            orphan_events: self.pending.orphan_events(),
            follow_subscribers: self.followers.len(),
            follow_dropped: self.follow_dropped,
            spilled: self.spilled,
            spill_path: spill
                .map(|spill| spill.active_or_next_path().to_string_lossy().into_owned()),
            spill_dir: spill.map(|spill| spill.dir.to_string_lossy().into_owned()),
            spill_bytes: spill.map(|spill| spill.bytes_on_disk).unwrap_or(0),
            spill_segments: spill.map(|spill| spill.segments.len()).unwrap_or(0),
            spill_segment_bytes: spill.map(|spill| spill.segment_bytes).unwrap_or(0),
            spill_disk_budget_bytes: spill.map(|spill| spill.disk_budget_bytes).unwrap_or(0),
            spill_compression: spill.map(|spill| spill.compression.name().to_string()),
            spill_evicted_segments: spill.map(|spill| spill.evicted_segments).unwrap_or(0),
            spill_errors: self.spill_errors,
            last_spill_error: self.last_spill_error.clone(),
            spill_index_entries: spill
                .map(|spill| {
                    spill
                        .segments
                        .iter()
                        .map(|segment| segment.indexed_records)
                        .sum()
                })
                .unwrap_or(0),
            spill_corrupt_records: self.spill_corrupt_records,
        }
    }

    fn spill_path(&mut self) -> Option<PathBuf> {
        self.ensure_spill();
        self.spill
            .as_ref()
            .map(TraceSpillState::active_or_next_path)
    }

    fn spill_paths(&mut self) -> Vec<PathBuf> {
        self.ensure_spill();
        self.spill
            .as_ref()
            .map(|spill| {
                spill
                    .segments
                    .iter()
                    .map(|segment| segment.path.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn follow(
        &mut self,
        after: u64,
        backlog_limit: usize,
        sender: SyncSender<Arc<Session>>,
        liveness: Weak<()>,
        reply: Sender<Vec<Arc<Session>>>,
    ) {
        let backlog = self.memory.list_after_shared(after, backlog_limit);
        if reply.send(backlog).is_ok() {
            self.followers.push(Follower { sender, liveness });
        }
    }

    fn publish(&mut self, session: Arc<Session>) {
        let mut active = Vec::with_capacity(self.followers.len());
        for follower in self.followers.drain(..) {
            if follower.liveness.strong_count() == 0 {
                continue;
            }
            match follower.sender.try_send(Arc::clone(&session)) {
                Ok(()) => active.push(follower),
                Err(TrySendError::Full(_)) => {
                    self.follow_dropped = self.follow_dropped.saturating_add(1);
                    active.push(follower);
                }
                Err(TrySendError::Disconnected(_)) => {}
            }
        }
        self.followers = active;
    }

    fn prune_followers(&mut self) {
        self.followers
            .retain(|follower| follower.liveness.strong_count() > 0);
    }

    fn enforce_memory_budget(&mut self, current_id: Option<u64>) {
        let budget = self.memory.memory_budget_bytes();
        let mut pending_bytes = self.pending.memory_bytes();
        if pending_bytes > budget
            && let Some(id) = current_id
        {
            self.pending.abort_for_budget(id);
            pending_bytes = self.pending.memory_bytes();
        }
        self.memory
            .evict_to_budget(budget.saturating_sub(pending_bytes));
    }

    fn ensure_spill(&mut self) {
        let result = self
            .spill
            .as_mut()
            .map(ensure_spill_initialized)
            .transpose();
        if let Err(error) = result {
            self.record_spill_error(error);
        }
    }

    fn record_spill_error(&mut self, error: io::Error) {
        self.spill_errors = self.spill_errors.saturating_add(1);
        self.last_spill_error = Some(error.to_string());
    }
}
