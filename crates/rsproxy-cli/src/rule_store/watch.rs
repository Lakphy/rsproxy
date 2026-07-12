use super::*;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const EVENT_QUEUE_CAPACITY: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct RuleWatchStatus {
    pub(crate) events: u64,
    pub(crate) dropped_events: u64,
    pub(crate) reloads: u64,
    pub(crate) failures: u64,
    pub(crate) last_reload_ms: Option<u64>,
    pub(crate) last_error: Option<String>,
}

pub(crate) struct RuleWatchHandle {
    watcher: Option<RecommendedWatcher>,
    messages: SyncSender<WatchMessage>,
    worker: Option<JoinHandle<()>>,
}

enum WatchMessage {
    Event(Event),
    Stop,
}

impl RuleStore {
    pub(crate) fn watch(&self, debounce: Duration) -> Result<RuleWatchHandle, RuleStoreError> {
        if debounce.is_zero() {
            return Err(RuleStoreError::Invalid(
                "rule watch debounce must be greater than zero".to_string(),
            ));
        }
        fs::create_dir_all(&self.inner.rules_dir)
            .map_err(|source| io_error("create rules directory", source))?;

        let (messages, receiver) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        let callback_messages = messages.clone();
        let callback_store = self.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            enqueue_event(&callback_store, &callback_messages, event);
        })
        .map_err(watch_error)?;
        watcher
            .watch(&self.inner.rules_dir, RecursiveMode::NonRecursive)
            .map_err(watch_error)?;

        let store = self.clone();
        let worker = thread::Builder::new()
            .name("rsproxy-rule-watch".to_string())
            .spawn(move || watch_loop(store, receiver, debounce))
            .map_err(|error| RuleStoreError::Watch(format!("spawn worker: {error}")))?;
        Ok(RuleWatchHandle {
            watcher: Some(watcher),
            messages,
            worker: Some(worker),
        })
    }

    pub(crate) fn watch_status(&self) -> RuleWatchStatus {
        self.inner
            .watch_status
            .lock()
            .expect("rule watch status poisoned")
            .clone()
    }

    fn reload_from_disk(&self) -> Result<bool, RuleStoreError> {
        let _update = self.inner.update.lock().expect("rule store poisoned");
        let next = load_snapshot(&self.inner.rules_dir)?;
        if self.snapshot().groups == next.groups {
            return Ok(false);
        }
        self.inner.snapshot.store(Arc::new(next));
        Ok(true)
    }

    fn record_event(&self) {
        let mut status = self
            .inner
            .watch_status
            .lock()
            .expect("rule watch status poisoned");
        status.events = status.events.saturating_add(1);
    }

    fn record_dropped_event(&self) {
        let mut status = self
            .inner
            .watch_status
            .lock()
            .expect("rule watch status poisoned");
        status.dropped_events = status.dropped_events.saturating_add(1);
    }

    fn record_reload(&self) {
        let mut status = self
            .inner
            .watch_status
            .lock()
            .expect("rule watch status poisoned");
        status.reloads = status.reloads.saturating_add(1);
        status.last_reload_ms = Some(rsproxy_trace::now_millis());
        status.last_error = None;
    }

    fn record_failure(&self, error: impl fmt::Display) {
        let mut status = self
            .inner
            .watch_status
            .lock()
            .expect("rule watch status poisoned");
        status.failures = status.failures.saturating_add(1);
        status.last_error = Some(error.to_string());
    }
}

impl Drop for RuleWatchHandle {
    fn drop(&mut self) {
        self.watcher.take();
        let _ = self.messages.send(WatchMessage::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn watch_loop(store: RuleStore, receiver: Receiver<WatchMessage>, debounce: Duration) {
    while let Ok(message) = receiver.recv() {
        match message {
            WatchMessage::Stop => return,
            WatchMessage::Event(event) if relevant(&store, &event) => {
                store.record_event();
                if !collect_burst(&store, &receiver, debounce) {
                    return;
                }
                match store.reload_from_disk() {
                    Ok(_) => store.record_reload(),
                    Err(error) => store.record_failure(error),
                }
            }
            WatchMessage::Event(_) => {}
        }
    }
}

fn collect_burst(store: &RuleStore, receiver: &Receiver<WatchMessage>, debounce: Duration) -> bool {
    loop {
        match receiver.recv_timeout(debounce) {
            Ok(WatchMessage::Stop) | Err(RecvTimeoutError::Disconnected) => return false,
            Err(RecvTimeoutError::Timeout) => return true,
            Ok(WatchMessage::Event(event)) if relevant(store, &event) => store.record_event(),
            Ok(WatchMessage::Event(_)) => {}
        }
    }
}

fn relevant(store: &RuleStore, event: &Event) -> bool {
    if matches!(event.kind, EventKind::Access(_)) {
        return false;
    }
    event.paths.is_empty()
        || event.paths.iter().any(|path| {
            path == &store.inner.rules_dir
                || path.file_name().and_then(|value| value.to_str()) == Some("groups.toml")
                || path.extension().and_then(|value| value.to_str()) == Some("rules")
        })
}

fn enqueue_event(
    store: &RuleStore,
    messages: &SyncSender<WatchMessage>,
    event: notify::Result<Event>,
) {
    let event = match event {
        Ok(event) if relevant(store, &event) => event,
        Ok(_) => return,
        Err(error) => {
            store.record_failure(error);
            return;
        }
    };
    match messages.try_send(WatchMessage::Event(event)) {
        Err(TrySendError::Full(_)) => store.record_dropped_event(),
        Err(TrySendError::Disconnected(_)) | Ok(()) => {}
    }
}

fn watch_error(error: notify::Error) -> RuleStoreError {
    RuleStoreError::Watch(error.to_string())
}

#[cfg(test)]
#[path = "watch/tests.rs"]
mod tests;
