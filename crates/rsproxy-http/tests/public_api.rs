//! Public facade smoke tests for shared HTTP protocol semantics.

use rsproxy_http::{
    is_forbidden_trailer_name, response_can_send_content, response_has_framed_body,
    status_can_send_content,
};

#[test]
fn public_http_semantics_distinguish_receive_framing_from_sender_content() {
    assert!(response_has_framed_body("GET", 205));
    assert!(!response_can_send_content("GET", 205));
    assert!(!status_can_send_content(205));
    assert!(is_forbidden_trailer_name("Content-Length"));
}
