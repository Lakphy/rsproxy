use super::*;
use http_body_util::{BodyExt, Full};
use std::thread;

#[test]
fn finish_freezes_the_first_observed_duration() {
    let timer = TransferTimer::start();
    thread::sleep(Duration::from_millis(20));

    let first = timer.finish();
    thread::sleep(Duration::from_millis(20));

    assert!(first >= 15);
    assert_eq!(timer.finish(), first);
    assert_eq!(timer.elapsed_ms(), Some(first));
}

#[test]
fn timed_body_finishes_when_the_stream_reaches_eof() {
    let timer = TransferTimer::start();
    let body = timed_body(
        Full::new(Bytes::from_static(b"payload")).boxed(),
        timer.clone(),
    );

    let collected = crate::h2::h2_runtime()
        .unwrap()
        .block_on(body.collect())
        .unwrap();

    assert_eq!(collected.to_bytes(), b"payload"[..]);
    assert!(timer.elapsed_ms().is_some());
}

#[test]
fn dropping_an_unconsumed_body_finishes_its_timer() {
    let timer = TransferTimer::start();
    let body = timed_body(
        Full::new(Bytes::from_static(b"payload")).boxed(),
        timer.clone(),
    );

    drop(body);

    assert!(timer.elapsed_ms().is_some());
}
