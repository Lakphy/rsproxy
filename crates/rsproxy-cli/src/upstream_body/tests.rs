use super::*;

#[test]
fn bounded_collection_preserves_unconsumed_data_for_streaming() {
    let mut stream = UpstreamBody::from_collected(
        b"abcdefgh".to_vec(),
        vec![("x-end".to_string(), "yes".to_string())],
    );

    assert_eq!(
        stream.collect_bounded(3).unwrap(),
        BoundedBody::Overflow {
            prefix: b"abc".to_vec()
        }
    );
    assert!(matches!(
        stream.next().unwrap().unwrap(),
        UpstreamBodyFrame::Data(data) if data == Bytes::from_static(b"defgh")
    ));
    assert!(matches!(
        stream.next().unwrap().unwrap(),
        UpstreamBodyFrame::Trailers(trailers)
            if trailers == vec![("x-end".to_string(), "yes".to_string())]
    ));
    assert!(stream.next().is_none());
}

#[test]
fn bounded_collection_returns_complete_body_and_trailers() {
    let stream = UpstreamBody::from_collected(
        b"body".to_vec(),
        vec![("x-end".to_string(), "yes".to_string())],
    );

    assert_eq!(
        stream.collect().unwrap(),
        CollectedBody {
            body: b"body".to_vec(),
            trailers: vec![("x-end".to_string(), "yes".to_string())]
        }
    );
}
