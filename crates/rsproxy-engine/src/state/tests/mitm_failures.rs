use super::*;
use std::time::{Duration, Instant};

#[test]
fn failure_entries_expire_at_the_configured_ttl() {
    let started = Instant::now();
    let mut cache = MitmFailureCache::new(4, Duration::from_secs(30));

    assert!(cache.remember_at("Pinned.Example.", started));
    assert!(cache.is_active_at("pinned.example", started + Duration::from_secs(29)));
    assert!(!cache.is_active_at("PINNED.EXAMPLE", started + Duration::from_secs(30)));
}

#[test]
fn failure_cache_uses_bounded_lru_eviction() {
    let started = Instant::now();
    let mut cache = MitmFailureCache::new(2, Duration::from_secs(30));

    cache.remember_at("a.test", started);
    cache.remember_at("b.test", started);
    assert!(cache.is_active_at("a.test", started + Duration::from_secs(1)));
    cache.remember_at("c.test", started + Duration::from_secs(2));

    assert!(cache.is_active_at("a.test", started + Duration::from_secs(2)));
    assert!(!cache.is_active_at("b.test", started + Duration::from_secs(2)));
    assert!(cache.is_active_at("c.test", started + Duration::from_secs(2)));
}

#[test]
fn zero_capacity_disables_failure_memory() {
    let started = Instant::now();
    let mut cache = MitmFailureCache::new(0, Duration::from_secs(30));

    assert!(!cache.remember_at("pinned.test", started));
    assert!(!cache.is_active_at("pinned.test", started));
}
