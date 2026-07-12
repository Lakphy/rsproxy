use super::*;
use std::fs;
use std::path::PathBuf;

mod collector;
mod events;
mod serialization;
mod spill_read;

#[test]
fn spill_writes_sessions_and_clear_removes_file() {
    let dir = temp_spill_dir("write-clear");
    let _ = fs::remove_dir_all(&dir);
    let store = TraceStore::new_with_spill_config(
        2,
        Some(TraceSpillConfig::new(
            dir.clone(),
            1024 * 1024,
            2 * 1024 * 1024,
        )),
    );
    let mut session = sample_session(1);
    session.url = "http://example.test/a".to_string();

    assert_eq!(store.record(session), 1);
    let stats = store.stats();
    assert_eq!(stats.spilled, 1);
    assert_eq!(stats.spill_errors, 0);
    assert_eq!(stats.spill_segments, 1);
    assert_eq!(
        stats.spill_dir.as_deref(),
        Some(dir.to_string_lossy().as_ref())
    );

    let paths = store.spill_paths();
    assert_eq!(paths.len(), 1);
    let body = fs::read_to_string(&paths[0]).expect("spill file should exist");
    assert!(body.contains("\"url\":\"http://example.test/a\""));
    assert!(body.contains("\"flags\":[\"cache\"]"));
    assert!(body.contains("\"pool_wait_ms\":3"));
    assert!(body.contains("\"dns_ms\":4"));
    assert!(body.contains("\"connect_ms\":5"));
    assert!(body.contains("\"ttfb_ms\":6"));
    assert!(body.contains("\"req_trailers\":[[\"x-request-end\",\"yes\"]]"));
    assert!(body.contains("[\"x-test\",\"yes\"]"));
    assert!(body.contains("\"tls\":[{\"phase\":\"upstream_tls\""));
    assert!(body.contains("\"peer_certificates\":2"));
    assert!(body.contains("\"cipher_suite\":\"TLS_AES_128_GCM_SHA256\""));
    assert!(body.contains("\"error\":null"));
    let index = fs::read_to_string(test_index_path_for_segment(&paths[0]))
        .expect("spill index should exist");
    assert_eq!(index.lines().count(), 1);
    let verified = String::from_utf8(store.spill_ndjson().unwrap()).unwrap();
    assert_eq!(verified, body);

    store.clear();
    assert!(store.spill_paths().is_empty());
    assert_eq!(store.stats().spilled, 0);
    assert_eq!(store.stats().spill_bytes, 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn spill_verified_reader_skips_crc_mismatches() {
    let dir = temp_spill_dir("crc-recovery");
    let _ = fs::remove_dir_all(&dir);
    let store = TraceStore::new_with_spill_config(
        4,
        Some(TraceSpillConfig::new(
            dir.clone(),
            1024 * 1024,
            2 * 1024 * 1024,
        )),
    );

    store.record(sample_session(1));
    store.record(sample_session(2));
    let path = store.spill_paths().pop().expect("spill path");
    let original = fs::read_to_string(&path).unwrap();
    assert!(original.contains("http://example.test/1"));
    assert!(original.contains("http://example.test/2"));

    let corrupt = original.replacen("http://example.test/2", "http://example.test/x", 1);
    fs::write(&path, corrupt).unwrap();

    let recovered = String::from_utf8(store.spill_ndjson().unwrap()).unwrap();
    assert!(recovered.contains("http://example.test/1"));
    assert!(!recovered.contains("http://example.test/2"));
    assert!(!recovered.contains("http://example.test/x"));
    let stats = store.stats();
    assert_eq!(stats.spill_index_entries, 2);
    assert_eq!(stats.spill_corrupt_records, 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn spill_zstd_segments_round_trip_with_verified_index() {
    let dir = temp_spill_dir("zstd-round-trip");
    let _ = fs::remove_dir_all(&dir);
    let config = TraceSpillConfig::new(dir.clone(), 1024 * 1024, 2 * 1024 * 1024)
        .with_compression(TraceSpillCompression::Zstd { level: 1 });
    let store = TraceStore::new_with_spill_config(4, Some(config.clone()));

    store.record(sample_session(1));
    store.record(sample_session(2));

    let stats = store.stats();
    assert_eq!(stats.spilled, 2);
    assert_eq!(stats.spill_compression.as_deref(), Some("zstd"));
    assert_eq!(stats.spill_index_entries, 2);
    assert!(
        stats
            .spill_path
            .as_deref()
            .unwrap_or_default()
            .ends_with(".ndjson.zst")
    );

    let path = store.spill_paths().pop().expect("compressed spill path");
    assert!(path.to_string_lossy().ends_with(".ndjson.zst"));
    let compressed = fs::read(&path).expect("compressed segment should exist");
    assert!(compressed.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]));
    assert!(!String::from_utf8_lossy(&compressed).contains("http://example.test/1"));

    let recovered = String::from_utf8(store.spill_ndjson().unwrap()).unwrap();
    assert!(recovered.contains("http://example.test/1"));
    assert!(recovered.contains("http://example.test/2"));

    let restarted = TraceStore::new_with_spill_config(4, Some(config));
    let restarted_stats = restarted.stats();
    assert_eq!(restarted_stats.spill_segments, 1);
    assert_eq!(restarted_stats.spill_index_entries, 2);
    assert_eq!(restarted_stats.spill_compression.as_deref(), Some("zstd"));
    let restarted_body = String::from_utf8(restarted.spill_ndjson().unwrap()).unwrap();
    assert_eq!(restarted_body, recovered);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn spill_zstd_reader_skips_corrupt_frames() {
    let dir = temp_spill_dir("zstd-corrupt");
    let _ = fs::remove_dir_all(&dir);
    let store = TraceStore::new_with_spill_config(
        4,
        Some(
            TraceSpillConfig::new(dir.clone(), 1024 * 1024, 2 * 1024 * 1024)
                .with_compression(TraceSpillCompression::Zstd { level: 1 }),
        ),
    );

    store.record(sample_session(1));
    store.record(sample_session(2));
    let path = store.spill_paths().pop().expect("compressed spill path");
    let index = fs::read_to_string(test_index_path_for_segment(&path)).unwrap();
    let second = index.lines().nth(1).expect("second index row");
    let entry = parse_index_entry(second).expect("valid index row");
    let mut data = fs::read(&path).unwrap();
    data[entry.offset] ^= 0xff;
    fs::write(&path, data).unwrap();

    let recovered = String::from_utf8(store.spill_ndjson().unwrap()).unwrap();
    assert!(recovered.contains("http://example.test/1"));
    assert!(!recovered.contains("http://example.test/2"));
    let stats = store.stats();
    assert_eq!(stats.spill_corrupt_records, 1);

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn spill_rotates_segments_and_enforces_disk_budget() {
    let dir = temp_spill_dir("rotate-budget");
    let _ = fs::remove_dir_all(&dir);
    let store =
        TraceStore::new_with_spill_config(8, Some(TraceSpillConfig::new(dir.clone(), 420, 950)));

    for id in 0..8 {
        assert_eq!(store.record(sample_session(id)), id + 1);
    }

    let stats = store.stats();
    assert_eq!(stats.spilled, 8);
    assert_eq!(stats.spill_errors, 0);
    assert!(stats.spill_segments < 8);
    assert!(stats.spill_evicted_segments > 0);
    assert!(stats.spill_bytes <= stats.spill_disk_budget_bytes);
    let paths = store.spill_paths();
    assert_eq!(paths.len(), stats.spill_segments);
    let body = paths
        .iter()
        .map(|path| fs::read_to_string(path).expect("segment should be readable"))
        .collect::<Vec<_>>()
        .join("");
    assert!(!body.contains("http://example.test/0"));
    assert!(body.contains("http://example.test/7"));

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn spill_stats_scan_existing_segments() {
    let dir = temp_spill_dir("scan-existing");
    let _ = fs::remove_dir_all(&dir);
    let config = TraceSpillConfig::new(dir.clone(), 1024 * 1024, 2 * 1024 * 1024);
    let store = TraceStore::new_with_spill_config(2, Some(config.clone()));
    store.record(sample_session(1));
    let written = store.stats().spill_bytes;
    assert!(written > 0);

    let restarted = TraceStore::new_with_spill_config(2, Some(config));
    let stats = restarted.stats();
    assert_eq!(stats.spill_segments, 1);
    assert_eq!(stats.spill_bytes, written);
    assert_eq!(stats.spill_index_entries, 1);
    assert!(
        stats
            .spill_path
            .unwrap()
            .ends_with("seg-000000000001.ndjson")
    );

    let _ = fs::remove_dir_all(&dir);
}

fn sample_session(id: u64) -> Session {
    let mut session = Session::new(
        SessionKind::Http,
        "GET".to_string(),
        format!("http://example.test/{id}"),
        "127.0.0.1:1".to_string(),
    );
    session.status = Some(200);
    session.pool_wait_ms = 3;
    session.dns_ms = 4;
    session.connect_ms = 5;
    session.ttfb_ms = 6;
    session.flags.push("cache".to_string());
    session
        .req_trailers
        .push(("x-request-end".to_string(), "yes".to_string()));
    session
        .res_headers
        .push(("x-test".to_string(), "yes".to_string()));
    session.res_body_head = vec![b'a'; 160];
    session.tls.push(TlsRecord {
        phase: "upstream_tls".to_string(),
        host: "example.test".to_string(),
        handshake_ms: 7,
        peer_certificates: 2,
        protocol: Some("TLSv1_3".to_string()),
        cipher_suite: Some("TLS_AES_128_GCM_SHA256".to_string()),
        alpn: Some("http/1.1".to_string()),
        error: None,
    });
    session
}

fn temp_spill_dir(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("rsproxy-trace-{name}-{}", now_millis()))
}
