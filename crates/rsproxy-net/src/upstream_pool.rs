use std::collections::HashMap;
use std::io;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
/// In-memory active-operation counts partitioned by pool key.
pub struct KeyedActivity {
    per_key: HashMap<String, usize>,
}

/// Mutable accounting required by the generic pool admission loop.
pub trait ActivityStore {
    /// Returns the active reservation count for `key`.
    fn active_for(&self, key: &str) -> usize;
    /// Adds one active reservation for `key`.
    fn reserve(&mut self, key: &str);
    /// Releases one reservation and removes a zero-valued key.
    fn release(&mut self, key: &str);
}

impl ActivityStore for KeyedActivity {
    fn active_for(&self, key: &str) -> usize {
        self.per_key.get(key).copied().unwrap_or(0)
    }

    fn reserve(&mut self, key: &str) {
        *self.per_key.entry(key.to_string()).or_default() += 1;
    }

    fn release(&mut self, key: &str) {
        let Some(active) = self.per_key.get_mut(key) else {
            return;
        };
        *active = active.saturating_sub(1);
        if *active == 0 {
            self.per_key.remove(key);
        }
    }
}

#[derive(Clone, Copy)]
/// Labels used to produce stable pool-wait diagnostics.
pub struct PoolWaitSpec {
    /// Stage prefix included in wait errors, such as `h2`.
    pub stage: &'static str,
    /// Human-readable name of the configured concurrency limit.
    pub limit_label: &'static str,
}

/// Waits for and reserves one keyed pool slot.
///
/// The timeout is measured from the caller-provided `started` instant, so time
/// already spent before entering this function remains part of the wait budget.
pub fn acquire_slot<T: ActivityStore>(
    inner: &Mutex<T>,
    available: &Condvar,
    key: &str,
    limit: usize,
    timeout: Duration,
    started: Instant,
    spec: PoolWaitSpec,
) -> io::Result<u64> {
    if limit == 0 || timeout.is_zero() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "{} pool_wait: {} and timeout must be greater than zero",
                spec.stage, spec.limit_label
            ),
        ));
    }
    let mut activity = inner.lock().map_err(|_| poisoned(spec))?;
    loop {
        if activity.active_for(key) < limit {
            activity.reserve(key);
            return Ok(duration_millis(started.elapsed()));
        }
        let Some(remaining) = timeout.checked_sub(started.elapsed()) else {
            return Err(wait_timeout(timeout, limit, spec));
        };
        let (next, result) = available
            .wait_timeout(activity, remaining)
            .map_err(|_| poisoned(spec))?;
        activity = next;
        if result.timed_out() && activity.active_for(key) >= limit {
            return Err(wait_timeout(timeout, limit, spec));
        }
    }
}

fn poisoned(spec: PoolWaitSpec) -> io::Error {
    io::Error::other(format!("{} pool_wait: pool lock poisoned", spec.stage))
}

fn wait_timeout(timeout: Duration, limit: usize, spec: PoolWaitSpec) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!(
            "{} pool_wait: timeout after {}ms ({} {limit})",
            spec.stage,
            duration_millis(timeout),
            spec.limit_label
        ),
    )
}

fn duration_millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}
