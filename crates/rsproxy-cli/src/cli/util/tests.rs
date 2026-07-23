use super::*;
use std::io::Cursor;

#[test]
fn bounded_utf8_reader_accepts_exact_input_and_rejects_size_or_encoding() {
    assert_eq!(
        read_utf8_bounded(Cursor::new(b"abc"), 3, "fixture", "read fixture").unwrap(),
        "abc"
    );
    let error = read_utf8_bounded(Cursor::new(b"abcd"), 3, "fixture", "read fixture").unwrap_err();
    assert!(error.to_string().contains("3-byte limit"));

    let error = read_utf8_bounded(Cursor::new([0xff]), 1, "fixture", "read fixture").unwrap_err();
    assert!(error.to_string().contains("invalid utf-8"));
}

#[test]
fn bounded_rule_file_reader_uses_the_same_exact_boundary() {
    let path = std::env::temp_dir().join(format!(
        "rsproxy-bounded-reader-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    std::fs::write(&path, "abcd").unwrap();
    assert_eq!(read_utf8_file_bounded(&path, 4, "fixture").unwrap(), "abcd");
    let error = read_utf8_file_bounded(&path, 3, "fixture").unwrap_err();
    assert!(error.to_string().contains("3-byte limit"));
    let _ = std::fs::remove_file(path);
}
