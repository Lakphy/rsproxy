use super::*;
use rsproxy_rules::Action;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT_STORAGE: AtomicU64 = AtomicU64::new(1);

fn temp_storage(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rsproxy-rule-watch-{name}-{}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis(),
        NEXT_STORAGE.fetch_add(1, Ordering::Relaxed)
    ))
}

fn request() -> rsproxy_rules::RequestMeta {
    rsproxy_rules::RequestMeta {
        method: "GET".to_string(),
        url: "http://example.test/".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: None,
        template: Default::default(),
    }
}

fn resolved_status(store: &RuleStore) -> u16 {
    match store.snapshot().compiled.resolve(&request()).actions[0].action {
        Action::Status(status) => status,
        ref action => panic!("expected status action, got {action:?}"),
    }
}

fn wait_until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if condition() {
            return;
        }
        thread::sleep(Duration::from_millis(20));
    }
    panic!("timed out waiting for {label}");
}

#[test]
fn disk_reload_is_atomic_and_skips_unchanged_snapshots() {
    let storage = temp_storage("reload");
    let store = RuleStore::load(&storage).unwrap();
    store
        .set_group(
            "default",
            "@language 3\nexample.test status(201)".to_string(),
        )
        .unwrap();
    let original = store.snapshot();
    let path = storage.join("rules/default.rules");

    fs::write(&path, "@language 3\nexample.test unknown()").unwrap();
    assert!(matches!(
        store.reload_from_disk(),
        Err(RuleStoreError::Parse(_))
    ));
    assert!(Arc::ptr_eq(&original, &store.snapshot()));

    fs::write(&path, "@language 3\nexample.test status(202)").unwrap();
    assert!(store.reload_from_disk().unwrap());
    assert_eq!(resolved_status(&store), 202);
    let current = store.snapshot();
    assert!(!store.reload_from_disk().unwrap());
    assert!(Arc::ptr_eq(&current, &store.snapshot()));

    let _ = fs::remove_dir_all(storage);
}

#[test]
fn watcher_debounces_changes_recovers_from_invalid_rules_and_stops_cleanly() {
    let storage = temp_storage("events");
    let store = RuleStore::load(&storage).unwrap();
    store
        .set_group(
            "default",
            "@language 3\nexample.test status(201)".to_string(),
        )
        .unwrap();
    let handle = store.watch(Duration::from_millis(30)).unwrap();
    let path = storage.join("rules/default.rules");

    fs::write(&path, "@language 3\nexample.test status(202)").unwrap();
    wait_until("valid rule reload", || resolved_status(&store) == 202);

    fs::write(&path, "@language 3\nexample.test unknown()").unwrap();
    wait_until("invalid rule failure", || {
        store.watch_status().failures >= 1
    });
    assert_eq!(resolved_status(&store), 202);
    assert!(store.watch_status().last_error.is_some());

    fs::write(&path, "@language 3\nexample.test status(203)").unwrap();
    wait_until("watcher recovery", || resolved_status(&store) == 203);
    let status = store.watch_status();
    assert!(status.events >= 3);
    assert!(status.reloads >= 2);
    assert!(status.last_reload_ms.is_some());
    assert_eq!(status.last_error, None);

    drop(handle);
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn watcher_rejects_zero_debounce() {
    let storage = temp_storage("zero");
    let store = RuleStore::load(&storage).unwrap();
    assert!(matches!(
        store.watch(Duration::ZERO),
        Err(RuleStoreError::Invalid(_))
    ));
    let _ = fs::remove_dir_all(storage);
}

#[test]
fn event_queue_filters_noise_and_reports_bounded_queue_overflow() {
    let storage = temp_storage("queue");
    let store = RuleStore::load(&storage).unwrap();
    let rules_dir = storage.join("rules");
    let (messages, receiver) = mpsc::sync_channel(1);

    enqueue_event(
        &store,
        &messages,
        Ok(Event::new(EventKind::Any).add_path(rules_dir.join("default.rules"))),
    );
    enqueue_event(
        &store,
        &messages,
        Ok(Event::new(EventKind::Any).add_path(rules_dir.join("other.rules"))),
    );
    assert_eq!(store.watch_status().dropped_events, 1);
    assert!(matches!(receiver.recv().unwrap(), WatchMessage::Event(_)));

    enqueue_event(
        &store,
        &messages,
        Ok(Event::new(EventKind::Any).add_path(rules_dir.join(".editor.tmp"))),
    );
    assert!(matches!(
        receiver.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));

    let _ = fs::remove_dir_all(storage);
}
