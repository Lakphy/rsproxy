use std::collections::HashMap;
use std::io;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct KeyedActivity {
    per_key: HashMap<String, usize>,
}

pub trait ActivityStore {
    fn active_for(&self, key: &str) -> usize;
    fn reserve(&mut self, key: &str);
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
pub struct PoolWaitSpec {
    pub stage: &'static str,
    pub limit_label: &'static str,
}

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
