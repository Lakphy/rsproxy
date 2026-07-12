use super::*;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[test]
fn spill_snapshot_read_does_not_block_collector_or_include_later_appends() {
    let (store, dir) = spill_store("snapshot-append", 1024 * 1024, 2 * 1024 * 1024);
    store.record(sample_session(1));
    let (ready, release, export) = blocked_snapshot_export(store.clone());
    ready.recv_timeout(Duration::from_secs(2)).unwrap();

    let progress_store = store.clone();
    let (progress_sender, progress) = mpsc::channel();
    let progress_worker = thread::spawn(move || {
        progress_store.record(sample_session(2));
        progress_sender.send(progress_store.stats()).unwrap();
    });
    let progressed = progress.recv_timeout(Duration::from_secs(2));
    release.send(()).unwrap();
    let body = String::from_utf8(export.join().unwrap().unwrap()).unwrap();
    progress_worker.join().unwrap();

    assert_eq!(progressed.unwrap().spilled, 2);
    assert!(body.contains("http://example.test/1"));
    assert!(!body.contains("http://example.test/2"));
    let latest = String::from_utf8(store.spill_ndjson().unwrap()).unwrap();
    assert!(latest.contains("http://example.test/1"));
    assert!(latest.contains("http://example.test/2"));
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn open_spill_snapshot_survives_concurrent_clear() {
    let (store, dir) = spill_store("snapshot-clear", 1024 * 1024, 2 * 1024 * 1024);
    store.record(sample_session(1));
    store.record(sample_session(2));
    let path = store.spill_paths().pop().unwrap();
    let original = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        original.replacen("http://example.test/2", "http://example.test/x", 1),
    )
    .unwrap();
    let (ready, release, export) = blocked_snapshot_export(store.clone());
    ready.recv_timeout(Duration::from_secs(2)).unwrap();

    store.clear();
    assert_eq!(store.stats().spilled, 0);
    release.send(()).unwrap();
    let body = String::from_utf8(export.join().unwrap().unwrap()).unwrap();

    assert!(body.contains("http://example.test/1"));
    assert!(!body.contains("http://example.test/2"));
    assert_eq!(store.stats().spill_corrupt_records, 0);
    assert!(store.spill_ndjson().unwrap().is_empty());
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn open_spill_snapshot_survives_budget_eviction_of_captured_segments() {
    let (store, dir) = spill_store("snapshot-evict", 420, 950);
    store.record(sample_session(0));
    let (ready, release, export) = blocked_snapshot_export(store.clone());
    ready.recv_timeout(Duration::from_secs(2)).unwrap();

    for id in 1..8 {
        store.record(sample_session(id));
    }
    let stats = store.stats();
    assert!(stats.spill_evicted_segments > 0);
    release.send(()).unwrap();
    let body = String::from_utf8(export.join().unwrap().unwrap()).unwrap();

    assert!(body.contains("http://example.test/0"));
    let latest = String::from_utf8(store.spill_ndjson().unwrap()).unwrap();
    assert!(!latest.contains("http://example.test/0"));
    assert!(latest.contains("http://example.test/7"));
    let _ = fs::remove_dir_all(dir);
}

fn spill_store(name: &str, segment_bytes: u64, disk_budget_bytes: u64) -> (TraceStore, PathBuf) {
    let dir = temp_spill_dir(name);
    let _ = fs::remove_dir_all(&dir);
    let store = TraceStore::new_with_spill_config(
        16,
        Some(TraceSpillConfig::new(
            dir.clone(),
            segment_bytes,
            disk_budget_bytes,
        )),
    );
    (store, dir)
}

type BlockedExport = (
    mpsc::Receiver<()>,
    mpsc::Sender<()>,
    thread::JoinHandle<std::io::Result<Vec<u8>>>,
);

fn blocked_snapshot_export(store: TraceStore) -> BlockedExport {
    let (ready_sender, ready) = mpsc::channel();
    let (release, release_receiver) = mpsc::channel();
    let export = thread::spawn(move || {
        store.spill_ndjson_with_snapshot_hook(|| {
            ready_sender.send(()).unwrap();
            release_receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap();
        })
    });
    (ready, release, export)
}
