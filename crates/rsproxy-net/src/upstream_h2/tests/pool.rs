use super::*;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn stream_lease_wait_timeout_does_not_leak_active_slot() {
    let pool_key = format!(
        "test-h2-lease-timeout-{}",
        NEXT_CONNECTOR_GENERATION.fetch_add(1, Ordering::Relaxed)
    );
    let held = acquire_lease(&pool_key, 1, Duration::from_secs(1), Instant::now()).unwrap();

    let error = acquire_lease(&pool_key, 1, Duration::from_millis(30), Instant::now()).unwrap_err();
    assert_eq!(
        error.to_string(),
        "upstream_h2 pool_wait: timeout after 30ms (active stream limit 1)"
    );
    assert_eq!(h2_pool().inner.lock().unwrap().active_for(&pool_key), 1);

    drop(held);
    assert_eq!(h2_pool().inner.lock().unwrap().active_for(&pool_key), 0);
}

#[test]
fn connector_claim_is_serialized_per_pool_key() {
    let pool_key = format!(
        "test-h2-connector-{}",
        NEXT_CONNECTOR_GENERATION.fetch_add(1, Ordering::Relaxed)
    );
    let mut first = acquire_lease(&pool_key, 2, Duration::from_secs(1), Instant::now()).unwrap();
    assert!(
        wait_for_entry_or_connector(
            &pool_key,
            &mut first,
            2,
            Duration::from_secs(1),
            Instant::now(),
        )
        .unwrap()
        .is_none()
    );
    assert!(first.connector_generation.is_some());

    let waiter_key = pool_key.clone();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut second =
            acquire_lease(&waiter_key, 2, Duration::from_secs(1), Instant::now()).unwrap();
        let result = wait_for_entry_or_connector(
            &waiter_key,
            &mut second,
            2,
            Duration::from_secs(1),
            Instant::now(),
        );
        tx.send((result.map(|entry| entry.is_none()), second))
            .unwrap();
    });

    std::thread::sleep(Duration::from_millis(40));
    assert!(rx.try_recv().is_err());
    drop(first);

    let (claimed, second) = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(claimed.unwrap());
    assert!(second.connector_generation.is_some());
    drop(second);
    let pool = h2_pool().inner.lock().unwrap();
    assert_eq!(pool.active_for(&pool_key), 0);
    assert!(!pool.connecting.contains_key(&pool_key));
}

#[test]
fn connector_wait_respects_pool_wait_timeout() {
    let pool_key = format!(
        "test-h2-connector-timeout-{}",
        NEXT_CONNECTOR_GENERATION.fetch_add(1, Ordering::Relaxed)
    );
    let mut first = acquire_lease(&pool_key, 2, Duration::from_secs(1), Instant::now()).unwrap();
    assert!(
        wait_for_entry_or_connector(
            &pool_key,
            &mut first,
            2,
            Duration::from_secs(1),
            Instant::now(),
        )
        .unwrap()
        .is_none()
    );
    let request = UpstreamH2Request {
        method: "GET".to_string(),
        uri: "https://example.test/timeout".to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        trailers: Vec::new(),
    };

    let error = match dispatch_buffered(
        &pool_key,
        request,
        test_config(
            2,
            Duration::from_millis(30),
            Duration::from_secs(1),
            request_deadline(),
        ),
    ) {
        Err(error) => error,
        Ok(_) => panic!("second connector unexpectedly bypassed the wait timeout"),
    };
    assert_eq!(
        error.to_string(),
        "upstream_h2 pool_wait: timeout after 30ms (active stream limit 2)"
    );
    {
        let pool = h2_pool().inner.lock().unwrap();
        assert_eq!(pool.active_for(&pool_key), 1);
        assert_eq!(
            pool.connecting.get(&pool_key).copied(),
            first.connector_generation
        );
    }
    drop(first);
    let pool = h2_pool().inner.lock().unwrap();
    assert_eq!(pool.active_for(&pool_key), 0);
    assert!(!pool.connecting.contains_key(&pool_key));
}
