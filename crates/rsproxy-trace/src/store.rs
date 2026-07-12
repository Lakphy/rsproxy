use crate::event::{SessionStart, TraceEvent};
use crate::model::Session;
use crate::spill::{TraceSpillConfig, read_verified_snapshot};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

mod config;
mod counters;
mod follow;
mod memory;
mod pending;
mod stats;
mod worker;

pub use config::{
    DEFAULT_TRACE_BODY_LIMIT, DEFAULT_TRACE_MEMORY_BUDGET, DEFAULT_TRACE_QUEUE_CAPACITY,
    DEFAULT_TRACE_QUEUE_MEMORY_BUDGET, TraceStoreConfig,
};
use counters::TraceCounters;
pub use follow::TraceFollow;
use memory::estimate_session_bytes;
pub use stats::TraceStats;
use worker::{Command, TraceWorker};

#[derive(Clone)]
pub struct TraceStore {
    handle: Arc<TraceHandle>,
}

struct TraceHandle {
    sender: SyncSender<Command>,
    counters: Arc<TraceCounters>,
    worker: Mutex<Option<JoinHandle<()>>>,
    max_sessions: usize,
    queue_capacity: usize,
    memory_budget_bytes: usize,
    queue_memory_budget_bytes: usize,
    resident_memory_budget_bytes: usize,
}

impl fmt::Debug for TraceStore {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TraceStore")
            .field("max_sessions", &self.handle.max_sessions)
            .field("queue_capacity", &self.handle.queue_capacity)
            .field("memory_budget_bytes", &self.handle.memory_budget_bytes)
            .field(
                "queue_memory_budget_bytes",
                &self.handle.queue_memory_budget_bytes,
            )
            .finish_non_exhaustive()
    }
}

impl TraceStore {
    pub fn new(max_sessions: usize) -> Self {
        Self::new_with_config(TraceStoreConfig {
            max_sessions,
            ..TraceStoreConfig::default()
        })
    }

    pub fn new_with_spill(max_sessions: usize, spill_path: Option<PathBuf>) -> Self {
        let config = spill_path.map(|path| {
            let dir = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf();
            TraceSpillConfig::new(dir, 64 * 1024 * 1024, 2 * 1024 * 1024 * 1024)
        });
        Self::new_with_spill_config(max_sessions, config)
    }

    pub fn new_with_spill_config(
        max_sessions: usize,
        spill_config: Option<TraceSpillConfig>,
    ) -> Self {
        Self::new_with_config(TraceStoreConfig {
            max_sessions,
            spill: spill_config,
            ..TraceStoreConfig::default()
        })
    }

    pub fn new_with_limits(
        max_sessions: usize,
        queue_capacity: usize,
        memory_budget_bytes: usize,
        spill_config: Option<TraceSpillConfig>,
    ) -> Self {
        Self::new_with_config(TraceStoreConfig {
            max_sessions,
            queue_capacity,
            memory_budget_bytes,
            queue_memory_budget_bytes: None,
            body_limit: DEFAULT_TRACE_BODY_LIMIT,
            spill: spill_config,
        })
    }

    pub fn new_with_config(config: TraceStoreConfig) -> Self {
        let (queue_memory_budget_bytes, resident_memory_budget_bytes) = config.memory_partitions();
        let TraceStoreConfig {
            max_sessions,
            queue_capacity,
            memory_budget_bytes,
            queue_memory_budget_bytes: _,
            body_limit,
            spill,
        } = config;
        let queue_capacity = queue_capacity.max(1);
        let counters = Arc::new(TraceCounters::new());
        let (sender, receiver) = mpsc::sync_channel(queue_capacity);
        let worker_counters = Arc::clone(&counters);
        let worker = thread::Builder::new()
            .name("rsproxy-trace-collector".to_string())
            .spawn(move || {
                TraceWorker::new(
                    max_sessions,
                    memory_budget_bytes,
                    resident_memory_budget_bytes,
                    queue_memory_budget_bytes,
                    body_limit,
                    spill,
                )
                .run(receiver, worker_counters, queue_capacity);
            })
            .expect("failed to start trace collector");
        Self {
            handle: Arc::new(TraceHandle {
                sender,
                counters,
                worker: Mutex::new(Some(worker)),
                max_sessions,
                queue_capacity,
                memory_budget_bytes,
                queue_memory_budget_bytes,
                resident_memory_budget_bytes,
            }),
        }
    }

    pub fn record(&self, mut session: Session) -> u64 {
        let id = self.handle.counters.next_id.fetch_add(1, Ordering::Relaxed);
        session.id = id;
        let queued_bytes = estimate_session_bytes(&session);
        let events = TraceEvent::from_session(session);
        self.submit(
            Command::Events {
                events,
                queued_bytes,
            },
            queued_bytes,
        );
        id
    }

    pub fn start(&self, start: SessionStart) -> u64 {
        let id = self.handle.counters.next_id.fetch_add(1, Ordering::Relaxed);
        let event = TraceEvent::Start { id, start };
        let queued_bytes = event.estimated_bytes();
        self.submit(
            Command::Event {
                event,
                queued_bytes,
            },
            queued_bytes,
        );
        id
    }

    pub fn finish(&self, session: Session) -> bool {
        if session.id == 0 {
            return false;
        }
        let queued_bytes = estimate_session_bytes(&session);
        let events = TraceEvent::continuation_from_session(session);
        self.submit(
            Command::Events {
                events,
                queued_bytes,
            },
            queued_bytes,
        )
    }

    pub fn emit(&self, event: TraceEvent) -> bool {
        let queued_bytes = event.estimated_bytes();
        self.submit(
            Command::Event {
                event,
                queued_bytes,
            },
            queued_bytes,
        )
    }

    pub fn abort(&self, id: u64) -> bool {
        self.emit(TraceEvent::Abort { id })
    }

    pub fn list(&self, limit: usize) -> Vec<Session> {
        self.query(|reply| Command::List { limit, reply })
            .unwrap_or_default()
    }

    pub fn list_after(&self, after: u64, limit: usize) -> Vec<Session> {
        self.query(|reply| Command::ListAfter {
            after,
            limit,
            reply,
        })
        .unwrap_or_default()
    }

    pub fn get(&self, id: u64) -> Option<Session> {
        self.query(|reply| Command::Get { id, reply })
            .unwrap_or(None)
    }

    pub fn clear(&self) {
        let _ = self.query(Command::Clear);
    }

    pub fn stats(&self) -> TraceStats {
        self.query(Command::Stats)
            .unwrap_or_else(|| self.empty_stats())
    }

    pub fn spill_path(&self) -> Option<PathBuf> {
        self.query(Command::SpillPath).unwrap_or(None)
    }

    pub fn spill_paths(&self) -> Vec<PathBuf> {
        self.query(Command::SpillPaths).unwrap_or_default()
    }

    pub fn spill_ndjson(&self) -> io::Result<Vec<u8>> {
        self.spill_ndjson_inner(|| {})
    }

    fn spill_ndjson_inner(&self, snapshot_ready: impl FnOnce()) -> io::Result<Vec<u8>> {
        let (generation, snapshot) = self.query(Command::SpillSnapshot).ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "trace collector is unavailable")
        })??;
        snapshot_ready();
        let (body, corrupt) = read_verified_snapshot(snapshot)?;
        let _ = self.query(|reply| Command::SpillReadReport {
            generation,
            corrupt,
            reply,
        });
        Ok(body)
    }

    pub fn follow(&self, after: u64, backlog_limit: usize, capacity: usize) -> Option<TraceFollow> {
        let (sender, receiver) = mpsc::sync_channel(capacity.max(1));
        let liveness = Arc::new(());
        let backlog = self.query(|reply| Command::Follow {
            after,
            backlog_limit,
            sender,
            liveness: Arc::downgrade(&liveness),
            reply,
        })?;
        Some(TraceFollow {
            backlog: backlog.into(),
            receiver,
            _liveness: liveness,
        })
    }

    fn query<T>(&self, command: impl FnOnce(mpsc::Sender<T>) -> Command) -> Option<T> {
        let (reply, response) = mpsc::channel();
        self.handle.sender.send(command(reply)).ok()?;
        response.recv().ok()
    }

    fn submit(&self, command: Command, queued_bytes: usize) -> bool {
        if !self
            .handle
            .counters
            .try_reserve(queued_bytes, self.handle.queue_memory_budget_bytes)
        {
            self.handle
                .counters
                .queue_dropped
                .fetch_add(1, Ordering::Relaxed);
            self.handle
                .counters
                .queue_memory_dropped
                .fetch_add(1, Ordering::Relaxed);
            return false;
        }
        match self.handle.sender.try_send(command) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.handle.counters.release_queue_bytes(queued_bytes);
                self.handle
                    .counters
                    .queue_dropped
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn block_collector(&self) -> CollectorBlock {
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel();
        self.handle
            .sender
            .send(Command::Block {
                started,
                release: release_rx,
            })
            .expect("trace collector is available");
        started_rx.recv().expect("trace collector entered block");
        CollectorBlock {
            release: Some(release),
        }
    }

    #[cfg(test)]
    pub(crate) fn spill_ndjson_with_snapshot_hook(
        &self,
        snapshot_ready: impl FnOnce(),
    ) -> io::Result<Vec<u8>> {
        self.spill_ndjson_inner(snapshot_ready)
    }
}

impl Drop for TraceHandle {
    fn drop(&mut self) {
        let (reply, response) = mpsc::channel();
        if self.sender.send(Command::Shutdown(reply)).is_ok() {
            let _ = response.recv();
        }
        if let Some(worker) = self.worker.lock().unwrap().take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
pub(crate) struct CollectorBlock {
    release: Option<mpsc::Sender<()>>,
}

#[cfg(test)]
impl Drop for CollectorBlock {
    fn drop(&mut self) {
        if let Some(release) = self.release.take() {
            let _ = release.send(());
        }
    }
}
