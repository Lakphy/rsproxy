use super::*;
use crate::upstream_pool::{ActivityStore, KeyedActivity, PoolWaitSpec, acquire_slot};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

pub(super) const H2_POOL_IDLE_TTL: Duration = Duration::from_secs(60);
const H2_POOL_CAPACITY: usize = 256;

#[derive(Debug)]
pub(crate) struct H2PoolLease {
    pub(super) key: String,
    pub(super) pool_wait_ms: u64,
    pub(super) connector_generation: Option<u64>,
}

#[derive(Clone)]
pub(super) struct PoolEntry {
    pub(super) generation: u64,
    pub(super) sender: H2Sender,
    pub(super) last_used: Instant,
}

#[derive(Default)]
pub(super) struct H2Pool {
    pub(super) entries: HashMap<String, PoolEntry>,
    order: VecDeque<String>,
    activity: KeyedActivity,
    pub(super) connecting: HashMap<String, u64>,
}

impl H2Pool {
    pub(super) fn get(&mut self, key: &str) -> Option<PoolEntry> {
        let now = Instant::now();
        if self.entries.get(key).is_some_and(|entry| {
            now.duration_since(entry.last_used) >= H2_POOL_IDLE_TTL && self.active_for(key) == 0
        }) {
            self.entries.remove(key);
            self.order.retain(|seen| seen != key);
            return None;
        }
        let entry = self.entries.get_mut(key)?;
        entry.last_used = now;
        let entry = entry.clone();
        self.touch(key);
        Some(entry)
    }

    pub(super) fn insert(&mut self, key: String, entry: PoolEntry) {
        self.connecting.remove(&key);
        if self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), entry);
            self.touch(&key);
            return;
        }
        while self.entries.len() >= H2_POOL_CAPACITY {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, entry);
    }

    fn remove_if_generation(&mut self, key: &str, generation: u64) {
        if self
            .entries
            .get(key)
            .is_some_and(|entry| entry.generation == generation)
        {
            self.entries.remove(key);
            self.order.retain(|seen| seen != key);
        }
    }

    fn touch(&mut self, key: &str) {
        self.order.retain(|seen| seen != key);
        self.order.push_back(key.to_string());
    }

    pub(super) fn active_for(&self, key: &str) -> usize {
        self.activity.active_for(key)
    }

    fn release(&mut self, key: &str) {
        self.activity.release(key);
    }

    fn claim_connector(&mut self, key: &str) -> Option<u64> {
        if self.connecting.contains_key(key) {
            return None;
        }
        let generation = NEXT_CONNECTOR_GENERATION.fetch_add(1, Ordering::Relaxed);
        self.connecting.insert(key.to_string(), generation);
        Some(generation)
    }

    fn cancel_connector(&mut self, key: &str, generation: u64) {
        if self.connecting.get(key).copied() == Some(generation) {
            self.connecting.remove(key);
        }
    }
}

impl Drop for H2PoolLease {
    fn drop(&mut self) {
        let state = h2_pool();
        let mut pool = state.inner.lock().expect("HTTP/2 pool lock poisoned");
        pool.release(&self.key);
        if let Some(generation) = self.connector_generation {
            pool.cancel_connector(&self.key, generation);
        }
        drop(pool);
        state.available.notify_all();
    }
}

pub(super) struct H2PoolState {
    pub(super) inner: Mutex<H2Pool>,
    pub(super) available: Condvar,
}

pub(super) static NEXT_CONNECTOR_GENERATION: AtomicU64 = AtomicU64::new(1);

pub(super) fn acquire_lease(
    pool_key: &str,
    max_active_streams_per_key: usize,
    timeout: Duration,
    started: Instant,
) -> io::Result<H2PoolLease> {
    let state = h2_pool();
    let pool_wait_ms = acquire_slot(
        &state.inner,
        &state.available,
        pool_key,
        max_active_streams_per_key,
        timeout,
        started,
        PoolWaitSpec {
            stage: "upstream_h2",
            limit_label: "active stream limit",
        },
    )?;
    Ok(H2PoolLease {
        key: pool_key.to_string(),
        pool_wait_ms,
        connector_generation: None,
    })
}

impl ActivityStore for H2Pool {
    fn active_for(&self, key: &str) -> usize {
        self.activity.active_for(key)
    }

    fn reserve(&mut self, key: &str) {
        self.activity.reserve(key);
    }

    fn release(&mut self, key: &str) {
        self.activity.release(key);
    }
}

pub(super) fn wait_for_entry_or_connector(
    pool_key: &str,
    lease: &mut H2PoolLease,
    max_active_streams_per_key: usize,
    timeout: Duration,
    started: Instant,
) -> io::Result<Option<PoolEntry>> {
    let state = h2_pool();
    let mut pool = state.inner.lock().expect("HTTP/2 pool lock poisoned");
    loop {
        if let Some(entry) = pool.get(pool_key) {
            return Ok(Some(entry));
        }
        if started.elapsed() >= timeout {
            return Err(pool_wait_timeout_error(timeout, max_active_streams_per_key));
        }
        if let Some(generation) = pool.claim_connector(pool_key) {
            lease.connector_generation = Some(generation);
            lease.pool_wait_ms = duration_millis(started.elapsed());
            return Ok(None);
        }
        let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
            return Err(pool_wait_timeout_error(timeout, max_active_streams_per_key));
        };
        let (next, result) = state
            .available
            .wait_timeout(pool, remaining)
            .map_err(|_| stage_error("pool_wait", "pool lock poisoned"))?;
        pool = next;
        if result.timed_out()
            && pool.get(pool_key).is_none()
            && pool.connecting.contains_key(pool_key)
        {
            return Err(pool_wait_timeout_error(timeout, max_active_streams_per_key));
        }
    }
}

fn pool_wait_timeout_error(timeout: Duration, max_active_streams_per_key: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "upstream_h2 pool_wait: timeout after {}ms (active stream limit {max_active_streams_per_key})",
            duration_millis(timeout)
        ),
    )
}

pub(super) fn h2_pool() -> &'static H2PoolState {
    static POOL: OnceLock<H2PoolState> = OnceLock::new();
    POOL.get_or_init(|| H2PoolState {
        inner: Mutex::new(H2Pool::default()),
        available: Condvar::new(),
    })
}

pub(super) fn remove_pool_entry(key: &str, generation: u64) {
    h2_pool()
        .inner
        .lock()
        .expect("HTTP/2 pool lock poisoned")
        .remove_if_generation(key, generation);
}

pub(super) fn spawn_idle_eviction(key: String, generation: u64) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(H2_POOL_IDLE_TTL).await;
            let mut pool = h2_pool().inner.lock().expect("HTTP/2 pool lock poisoned");
            let Some(entry) = pool.entries.get(&key) else {
                return;
            };
            if entry.generation != generation {
                return;
            }
            if entry.last_used.elapsed() >= H2_POOL_IDLE_TTL && pool.active_for(&key) == 0 {
                pool.remove_if_generation(&key, generation);
                return;
            }
        }
    });
}
