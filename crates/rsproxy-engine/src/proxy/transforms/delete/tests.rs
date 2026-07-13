use super::*;

#[test]
fn path_segment_delete_uses_original_indexes_and_distinguishes_last() {
    let mut path = "/api/path/to/item".to_string();
    delete_path_segments(
        &mut path,
        &[DeletePathSegment::Index(0), DeletePathSegment::Index(-1)],
    );
    assert_eq!(path, "/path/to");

    let mut path = "/api/path/to/item".to_string();
    delete_path_segments(&mut path, &[DeletePathSegment::Last]);
    assert_eq!(path, "/api/path/to/");
}

#[test]
fn url_delete_can_clear_path_or_query_independently() {
    let mut url = UrlParts::parse("http://example.test/api/item?drop=1&keep=2").unwrap();
    apply_url_delete(
        &mut url,
        &[DeleteOp::Pathname, DeleteOp::UrlParam("drop".to_string())],
    );
    assert_eq!(url.path, "/");
    assert_eq!(url.query.as_deref(), Some("keep=2"));

    apply_url_delete(&mut url, &[DeleteOp::UrlParams]);
    assert_eq!(url.query, None);
}

#[test]
fn content_type_delete_matches_type_and_charset_contracts() {
    let mut headers = vec![(
        "Content-Type".to_string(),
        "application/json; charset=utf-8; boundary=x".to_string(),
    )];
    remove_content_type_part(&mut headers, true);
    assert_eq!(
        http::header(&headers, "content-type"),
        Some("; charset=utf-8; boundary=x")
    );

    remove_content_type_part(&mut headers, false);
    assert_eq!(http::header(&headers, "content-type"), None);
}
