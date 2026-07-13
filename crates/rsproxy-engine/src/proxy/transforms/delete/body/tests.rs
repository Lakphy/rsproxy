use super::*;

fn path(segments: Vec<DeleteBodyPathSegment>) -> DeleteBodyPath {
    DeleteBodyPath::new(segments).unwrap()
}

fn key(value: &str) -> DeleteBodyPathSegment {
    DeleteBodyPathSegment::Key(value.to_string())
}

#[test]
fn request_json_delete_handles_nested_objects_arrays_and_literal_dots() {
    let headers = vec![(
        "Content-Type".to_string(),
        "application/problem+json; charset=utf-8".to_string(),
    )];
    let mut body = br#"{
        "profile":{"secret":true,"keep":1},
        "items":[{"name":"first"},{"name":"second"}],
        "meta":{"a.b":"drop","keep":"yes"}
    }"#
    .to_vec();

    assert!(delete_request_body_path(
        &headers,
        &mut body,
        &path(vec![key("profile"), key("secret")]),
    ));
    assert!(delete_request_body_path(
        &headers,
        &mut body,
        &path(vec![key("items"), DeleteBodyPathSegment::Index(0)]),
    ));
    assert!(delete_request_body_path(
        &headers,
        &mut body,
        &path(vec![key("meta"), key("a.b")]),
    ));

    let value: JsonValue = serde_json::from_slice(&body).unwrap();
    assert_eq!(value["profile"], serde_json::json!({"keep": 1}));
    assert_eq!(value["items"], serde_json::json!([{"name": "second"}]));
    assert_eq!(value["meta"], serde_json::json!({"keep": "yes"}));
}

#[test]
fn request_form_delete_preserves_unmatched_fields_and_duplicate_order() {
    let headers = vec![(
        "Content-Type".to_string(),
        "application/x-www-form-urlencoded".to_string(),
    )];
    let mut body = b"drop=1&keep=first&profile.secret=2&keep=second".to_vec();

    assert!(delete_request_body_path(
        &headers,
        &mut body,
        &path(vec![key("drop")]),
    ));
    assert!(delete_request_body_path(
        &headers,
        &mut body,
        &path(vec![key("profile"), key("secret")]),
    ));
    assert_eq!(body, b"keep=first&keep=second");
}

#[test]
fn response_jsonp_delete_preserves_the_callback_wrapper() {
    let headers = vec![(
        "Content-Type".to_string(),
        "application/javascript".to_string(),
    )];
    let mut body = br#"callbacks[0]({"payload":{"secret":1,"keep":2},"items":[0,1,2]});"#.to_vec();

    assert!(delete_response_body_path(
        &headers,
        &mut body,
        &path(vec![key("payload"), key("secret")]),
    ));
    assert!(delete_response_body_path(
        &headers,
        &mut body,
        &path(vec![key("items"), DeleteBodyPathSegment::Index(1)]),
    ));

    let text = std::str::from_utf8(&body).unwrap();
    assert!(text.starts_with("callbacks[0]("));
    assert!(text.ends_with(");"));
    let value: JsonValue = serde_json::from_str(&text[13..text.len() - 2]).unwrap();
    assert_eq!(value["payload"], serde_json::json!({"keep": 2}));
    assert_eq!(value["items"], serde_json::json!([0, 2]));
}

#[test]
fn body_delete_is_a_noop_for_missing_paths_compression_and_wrong_media_types() {
    let missing = path(vec![key("missing")]);
    let mut body = b"{ \"keep\": 1 }".to_vec();
    let original = body.clone();
    assert!(!delete_request_body_path(
        &[("Content-Type".to_string(), "application/json".to_string())],
        &mut body,
        &missing,
    ));
    assert_eq!(body, original);

    assert!(!delete_request_body_path(
        &[
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Content-Encoding".to_string(), "gzip".to_string()),
        ],
        &mut body,
        &path(vec![key("keep")]),
    ));
    assert!(!delete_response_body_path(
        &[("Content-Type".to_string(), "text/plain".to_string())],
        &mut body,
        &path(vec![key("keep")]),
    ));
    assert_eq!(body, original);
}
