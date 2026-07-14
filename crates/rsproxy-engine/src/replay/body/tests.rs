use super::read_response_body;
use std::io::Cursor;

#[test]
fn decodes_chunks_and_bounds_large_previews() {
    let mut chunked = Cursor::new(b"3\r\nabc\r\n4\r\ndefg\r\n0\r\nX-End: yes\r\n\r\n");
    let headers = vec![("Transfer-Encoding".to_string(), "chunked".to_string())];
    let (size, preview) = read_response_body(&mut chunked, "GET", 200, &headers, 1024, 8).unwrap();
    assert_eq!(size, 7);
    assert_eq!(preview, b"abcdefg");

    let body = vec![b'x'; 70 * 1024];
    let headers = vec![("Content-Length".to_string(), body.len().to_string())];
    let (size, preview) =
        read_response_body(&mut Cursor::new(&body), "GET", 200, &headers, 1024, 8).unwrap();
    assert_eq!(size, body.len());
    assert_eq!(preview.len(), 64 * 1024);
}
