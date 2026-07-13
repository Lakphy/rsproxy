use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct MitmFailureCache {
    capacity: usize,
    ttl: Duration,
    entries: HashMap<String, Instant>,
    order: VecDeque<String>,
}

impl MitmFailureCache {
    pub(crate) fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity,
            ttl,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(crate) fn is_active(&mut self, host: &str) -> bool {
        self.is_active_at(host, Instant::now())
    }

    pub(crate) fn remember(&mut self, host: &str) -> bool {
        self.remember_at(host, Instant::now())
    }

    pub(crate) fn clear(&mut self, host: &str) {
        let host = cache_key(host);
        self.entries.remove(&host);
        self.order.retain(|seen| seen != &host);
    }

    pub(crate) fn active_len(&mut self) -> usize {
        self.purge_expired(Instant::now());
        self.entries.len()
    }

    fn purge_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.entries
            .retain(|_, failed_at| now.saturating_duration_since(*failed_at) < ttl);
        self.order.retain(|host| self.entries.contains_key(host));
    }

    fn touch(&mut self, host: &str) {
        self.order.retain(|seen| seen != host);
        self.order.push_back(host.to_string());
    }

    pub(super) fn is_active_at(&mut self, host: &str, now: Instant) -> bool {
        self.purge_expired(now);
        let host = cache_key(host);
        if !self.entries.contains_key(&host) {
            return false;
        }
        self.touch(&host);
        true
    }

    pub(super) fn remember_at(&mut self, host: &str, now: Instant) -> bool {
        if self.capacity == 0 || self.ttl.is_zero() {
            return false;
        }
        self.purge_expired(now);
        let host = cache_key(host);
        self.entries.insert(host.clone(), now);
        self.touch(&host);
        while self.entries.len() > self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        true
    }
}

fn cache_key(host: &str) -> String {
    host.trim_end_matches('.').to_ascii_lowercase()
}
