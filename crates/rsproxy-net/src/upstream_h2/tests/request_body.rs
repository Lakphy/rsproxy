use super::*;
use http_body_util::BodyExt;

#[test]
fn channel_request_body_preserves_data_and_trailers() {
    let (sender, body) = crate::upstream_h2::request_body::request_body_channel(4096, 16);
    assert!(
        sender
            .send_data(Bytes::from_static(b"streamed"), request_deadline())
            .unwrap()
    );
    assert!(
        sender
            .send_trailers(
                vec![("x-upload-end".to_string(), "done".to_string())],
                request_deadline(),
            )
            .unwrap()
    );
    drop(sender);

    let collected = h2_runtime().unwrap().block_on(body.collect()).unwrap();

    let trailers = collected.trailers().cloned().unwrap();
    assert_eq!(collected.to_bytes(), Bytes::from_static(b"streamed"));
    assert_eq!(trailers["x-upload-end"], "done");
}

#[test]
fn channel_request_body_preserves_input_error() {
    let (sender, body) = crate::upstream_h2::request_body::request_body_channel(4096, 16);
    sender
        .send_error(
            &io::Error::new(io::ErrorKind::InvalidData, "invalid upload"),
            request_deadline(),
        )
        .unwrap();
    drop(sender);

    let error = h2_runtime().unwrap().block_on(body.collect()).unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("invalid upload"));
}

#[test]
fn channel_request_body_applies_bounded_deadline_backpressure() {
    let (sender, _body) = crate::upstream_h2::request_body::request_body_channel(4096, 16);
    let deadline = RequestDeadline::new(Duration::from_millis(200)).unwrap();
    for _ in 0..8 {
        assert!(
            sender
                .send_data(Bytes::from_static(b"buffered"), deadline)
                .unwrap()
        );
    }

    let error = sender
        .send_data(Bytes::from_static(b"blocked"), deadline)
        .unwrap_err();

    assert!(is_request_total_timeout(&error));
}

#[test]
fn channel_request_body_rejects_forbidden_trailer() {
    let (sender, _body) = crate::upstream_h2::request_body::request_body_channel(4096, 16);

    let error = sender
        .send_trailers(
            vec![("content-length".to_string(), "1".to_string())],
            request_deadline(),
        )
        .unwrap_err();

    assert!(error.to_string().contains("forbidden request trailer"));
}
