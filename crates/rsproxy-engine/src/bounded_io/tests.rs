use super::*;

#[test]
fn bounded_file_reader_accepts_exact_size_and_rejects_one_extra_byte() {
    let path = std::env::temp_dir().join(format!(
        "rsproxy-engine-bounded-file-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    std::fs::write(&path, b"abcd").unwrap();
    assert_eq!(read_file(&path, 4, "fixture").unwrap(), b"abcd");
    let error = read_file(&path, 3, "fixture").unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("3-byte limit"));
    let _ = std::fs::remove_file(path);
}
