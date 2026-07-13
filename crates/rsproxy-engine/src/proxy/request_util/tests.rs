use super::*;

#[test]
fn throttle_pacer_carries_the_rate_limit_across_separate_writes() {
    let mut output = Vec::new();
    let mut pacer = ThrottlePacer::new(Some(1024 * 1024));
    pacer.write(&mut output, &[b'a'; 16 * 1024]).unwrap();

    let started = Instant::now();
    pacer.write(&mut output, b"b").unwrap();

    assert!(started.elapsed() >= Duration::from_millis(8));
    assert_eq!(output.len(), 16 * 1024 + 1);
}

#[test]
fn throttle_pacer_obeys_the_absolute_request_deadline() {
    let mut output = Vec::new();
    let mut pacer = ThrottlePacer::new(Some(1));
    pacer
        .write_until(
            &mut output,
            b"a",
            RequestDeadline::new(Duration::from_millis(20)).unwrap(),
        )
        .unwrap();
    let deadline = RequestDeadline::new(Duration::from_millis(20)).unwrap();

    let error = pacer.write_until(&mut output, b"b", deadline).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(output, b"a");
}

#[test]
fn throttle_pacer_normalizes_programmatic_zero_rates() {
    let pacer = ThrottlePacer::new(Some(0));

    assert_eq!(pacer.bytes_per_sec, Some(1));
}
