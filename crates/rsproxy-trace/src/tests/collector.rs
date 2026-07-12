use super::*;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[test]
fn record_drops_at_capacity_without_waiting_for_collector() {
    let store = TraceStore::new_with_limits(8, 2, 1024 * 1024, None);
    let blocked = store.block_collector();

    assert_eq!(store.record(sample_session(0)), 1);
    assert_eq!(store.record(sample_session(1)), 2);
    assert_eq!(store.record(sample_session(2)), 3);

    drop(blocked);
    let stats = store.stats();
    assert_eq!(stats.sessions, 2);
    assert_eq!(stats.queue_capacity, 2);
    assert_eq!(stats.queue_dropped, 1);
    assert_eq!(stats.dropped, 1);
    assert_eq!(stats.next_id, 4);
    assert_eq!(
        store
            .list(8)
            .iter()
            .map(|session| session.id)
            .collect::<Vec<_>>(),
        vec![2, 1]
    );
}

#[test]
fn query_commands_are_barriers_for_accepted_records() {
    let store = TraceStore::new_with_limits(8, 8, 1024 * 1024, None);

    let id = store.record(sample_session(7));

    assert_eq!(store.get(id).unwrap().url, "http://example.test/7");
    assert_eq!(store.list_after(0, 8).len(), 1);
    assert_eq!(store.stats().sessions, 1);
}

#[test]
fn memory_budget_evicts_oldest_complete_sessions() {
    let probe = TraceStore::new_with_limits(2, 8, 1024 * 1024, None);
    probe.record(sample_session(0));
    let one_session_bytes = probe.stats().memory_bytes;
    drop(probe);

    let resident_budget = one_session_bytes + 1;
    let queue_budget = one_session_bytes + 1;
    let store = TraceStore::new_with_config(TraceStoreConfig {
        max_sessions: 8,
        queue_capacity: 8,
        memory_budget_bytes: resident_budget + queue_budget,
        queue_memory_budget_bytes: Some(queue_budget),
        body_limit: DEFAULT_TRACE_BODY_LIMIT,
        spill: None,
    });
    store.record(sample_session(1));
    assert_eq!(store.stats().sessions, 1);
    store.record(sample_session(2));
    let stats = store.stats();

    assert_eq!(stats.sessions, 1);
    assert_eq!(stats.evicted_sessions, 1);
    assert!(stats.memory_bytes <= stats.memory_budget_bytes);
    assert_eq!(stats.resident_memory_budget_bytes, resident_budget);
    assert_eq!(store.list(1)[0].url, "http://example.test/2");
}

#[test]
fn session_larger_than_memory_budget_is_dropped_before_queueing() {
    let store = TraceStore::new_with_limits(8, 8, 1, None);

    store.record(sample_session(1));
    let stats = store.stats();

    assert_eq!(stats.sessions, 0);
    assert_eq!(stats.memory_bytes, 0);
    assert_eq!(stats.evicted_sessions, 0);
    assert_eq!(stats.queue_memory_dropped, 1);
    assert_eq!(stats.queue_dropped, 1);
}

#[test]
fn queue_memory_budget_counts_reserved_container_capacity() {
    let store = TraceStore::new_with_limits(8, 8, 64 * 1024, None);
    let mut method = String::with_capacity(128 * 1024);
    method.push_str("GET");
    let session = Session::new(
        SessionKind::Http,
        method,
        "http://example.test/".to_string(),
        "127.0.0.1:12345".to_string(),
    );

    store.record(session);
    let stats = store.stats();

    assert_eq!(stats.sessions, 0);
    assert_eq!(stats.memory_bytes, 0);
    assert_eq!(stats.evicted_sessions, 0);
    assert_eq!(stats.queue_memory_dropped, 1);
}

#[test]
fn concurrent_producers_assign_unique_ids_without_queue_loss() {
    let store = TraceStore::new_with_limits(1024, 1024, 16 * 1024 * 1024, None);
    let ids = Arc::new(Mutex::new(Vec::new()));
    let mut workers = Vec::new();
    for worker in 0..8 {
        let store = store.clone();
        let ids = Arc::clone(&ids);
        workers.push(std::thread::spawn(move || {
            for index in 0..64 {
                ids.lock()
                    .unwrap()
                    .push(store.record(sample_session(worker * 64 + index)));
            }
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }

    let stats = store.stats();
    let ids = ids.lock().unwrap();
    let unique = ids.iter().copied().collect::<HashSet<_>>();
    assert_eq!(ids.len(), 512);
    assert_eq!(unique.len(), ids.len());
    assert_eq!(stats.sessions, 512);
    assert_eq!(stats.queue_dropped, 0);
}

#[test]
fn final_handle_drop_flushes_accepted_spill_records() {
    let dir = temp_spill_dir("collector-drop");
    let _ = fs::remove_dir_all(&dir);
    {
        let store = TraceStore::new_with_limits(
            8,
            8,
            1024 * 1024,
            Some(TraceSpillConfig::new(
                dir.clone(),
                1024 * 1024,
                2 * 1024 * 1024,
            )),
        );
        store.record(sample_session(42));
    }

    let body = fs::read_dir(&dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "ndjson")
        })
        .map(|path| fs::read_to_string(path).unwrap())
        .collect::<String>();
    assert!(body.contains("http://example.test/42"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn follow_transitions_from_ordered_backlog_to_live_records_without_a_gap() {
    let store = TraceStore::new_with_limits(8, 8, 1024 * 1024, None);
    store.record(sample_session(1));
    let mut follow = store.follow(0, 8, 2).unwrap();

    store.record(sample_session(2));

    assert_eq!(follow.try_recv().unwrap().url, "http://example.test/1");
    assert_eq!(
        follow.recv_timeout(Duration::from_millis(100)).unwrap().url,
        "http://example.test/2"
    );
    assert_eq!(store.stats().follow_subscribers, 1);
}

#[test]
fn slow_followers_drop_their_own_live_items_without_blocking_records() {
    let store = TraceStore::new_with_limits(8, 8, 1024 * 1024, None);
    let mut follow = store.follow(0, 0, 1).unwrap();

    store.record(sample_session(1));
    store.record(sample_session(2));
    let stats = store.stats();

    assert_eq!(stats.sessions, 2);
    assert_eq!(stats.follow_subscribers, 1);
    assert_eq!(stats.follow_dropped, 1);
    assert_eq!(follow.try_recv().unwrap().url, "http://example.test/1");
}

#[test]
fn dropped_followers_are_removed_before_the_next_stats_snapshot() {
    let store = TraceStore::new_with_limits(8, 8, 1024 * 1024, None);
    let follow = store.follow(0, 0, 1).unwrap();
    drop(follow);

    assert_eq!(store.stats().follow_subscribers, 0);
}

#[test]
fn collector_shutdown_disconnects_follow_receivers() {
    let store = TraceStore::new_with_limits(8, 8, 1024 * 1024, None);
    let mut follow = store.follow(0, 0, 1).unwrap();

    drop(store);

    assert!(matches!(
        follow.recv_timeout(Duration::from_millis(50)),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected)
    ));
}
