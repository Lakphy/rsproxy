use rsproxy_net::{NetError, NetStage, ProtocolErrorKind};
use std::error::Error as _;
use std::io;

#[test]
fn io_error_preserves_its_source() {
    let error = NetError::Io {
        context: "read response head".to_string(),
        source: io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed"),
    };

    let source = error.source().expect("I/O source should be retained");
    assert_eq!(source.to_string(), "peer closed");
    assert_eq!(error.to_string(), "read response head: peer closed");
}

#[test]
fn protocol_error_uses_typed_categories() {
    let error = NetError::Protocol {
        kind: ProtocolErrorKind::InvalidFraming,
        stage: NetStage::Response,
        message: "conflicting content lengths".to_string(),
    };

    assert_eq!(
        error.to_string(),
        "invalid framing during response transfer: conflicting content lengths"
    );
}
