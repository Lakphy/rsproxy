use crate::app::{api_display, unix_api_path};

#[test]
fn unix_api_endpoint_parsing_is_explicit() {
    assert_eq!(
        unix_api_path("unix:/tmp/rsproxy.sock"),
        Some("/tmp/rsproxy.sock")
    );
    assert_eq!(
        unix_api_path("unix:///tmp/rsproxy.sock"),
        Some("/tmp/rsproxy.sock")
    );
    assert_eq!(unix_api_path("127.0.0.1:8900"), None);
    assert_eq!(api_display("127.0.0.1:8900"), "http://127.0.0.1:8900");
    assert_eq!(
        api_display("unix:/tmp/rsproxy.sock"),
        "unix:/tmp/rsproxy.sock"
    );
    assert_eq!(
        crate::app::windows_pipe_path("pipe:rsproxy"),
        Some("rsproxy")
    );
    assert_eq!(
        crate::app::windows_pipe_path(r"npipe:\\.\pipe\rsproxy"),
        Some(r"\\.\pipe\rsproxy")
    );
    assert_eq!(crate::app::windows_pipe_path("127.0.0.1:8900"), None);
    assert_eq!(api_display("pipe:rsproxy"), "pipe:rsproxy");
}
