use super::super::*;

#[test]
fn control_disconnect_classification_keeps_expected_closes_out_of_warn_logs() {
    for kind in [
        std::io::ErrorKind::BrokenPipe,
        std::io::ErrorKind::ConnectionReset,
        std::io::ErrorKind::ConnectionAborted,
        std::io::ErrorKind::NotConnected,
        std::io::ErrorKind::UnexpectedEof,
    ] {
        assert!(expected_client_disconnect(&std::io::Error::from(kind)));
    }

    for kind in [
        std::io::ErrorKind::InvalidData,
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::TimedOut,
    ] {
        assert!(!expected_client_disconnect(&std::io::Error::from(kind)));
    }
}
