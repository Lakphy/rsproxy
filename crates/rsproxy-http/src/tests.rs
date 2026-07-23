use super::*;

#[test]
fn receive_framing_and_sender_content_are_distinct_for_205() {
    assert!(response_has_framed_body("GET", 205));
    assert!(!response_can_send_content("GET", 205));
    assert!(!status_can_send_content(205));
}

#[test]
fn method_and_status_exclusions_cover_head_connect_and_bodyless_statuses() {
    assert!(!response_has_framed_body("HEAD", 200));
    assert!(!response_has_framed_body("CONNECT", 200));
    assert!(response_has_framed_body("CONNECT", 400));
    for status in [100, 199, 204, 304] {
        assert!(!response_has_framed_body("GET", status));
        assert!(!response_can_send_content("GET", status));
    }
}

#[test]
fn trailer_policy_allows_extension_and_grpc_metadata() {
    assert!(is_forbidden_trailer_name("Content-Length"));
    assert!(is_forbidden_trailer_name("Authorization"));
    assert!(!is_forbidden_trailer_name("x-checksum"));
    assert!(!is_forbidden_trailer_name("grpc-status"));
}
