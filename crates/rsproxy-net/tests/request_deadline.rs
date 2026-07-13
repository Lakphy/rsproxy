use rsproxy_net::{RequestDeadline, is_request_total_timeout};
use std::io;
use std::time::{Duration, Instant};

#[test]
fn stage_budget_keeps_the_shorter_timeout_source() {
    let deadline = RequestDeadline::new(Duration::from_millis(80)).unwrap();
    let stage = deadline.budget(Duration::from_millis(20)).unwrap();
    assert!(stage.timeout() <= Duration::from_millis(20));
    let error = stage.timeout_error(|timeout| {
        io::Error::new(
            io::ErrorKind::TimedOut,
            format!("stage=test: timeout after {}ms", timeout.as_millis()),
        )
    });
    assert!(error.to_string().starts_with("stage=test: timeout after "));

    let deadline = RequestDeadline::new(Duration::from_millis(20)).unwrap();
    let total = deadline.budget(Duration::from_secs(1)).unwrap();
    assert_eq!(
        total
            .timeout_error(|_| io::Error::new(io::ErrorKind::TimedOut, "stage timeout"))
            .to_string(),
        "stage=request_total: timeout after 20ms"
    );
}

#[test]
fn sleep_uses_one_absolute_deadline() {
    let deadline = RequestDeadline::new(Duration::from_millis(30)).unwrap();
    let started = Instant::now();
    let error = deadline
        .sleep(Duration::from_millis(100))
        .expect_err("sleep should stop at the request deadline");

    assert!(is_request_total_timeout(&error));
    assert!(started.elapsed() >= Duration::from_millis(20));
    assert!(started.elapsed() < Duration::from_millis(500));
}

#[test]
fn zero_timeout_is_rejected() {
    let error = RequestDeadline::new(Duration::ZERO).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert_eq!(
        error.to_string(),
        "stage=request_total: timeout must be greater than zero"
    );
}
