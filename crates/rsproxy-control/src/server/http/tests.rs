use super::*;
use std::io::Cursor;

#[test]
fn parses_content_length_and_chunked_control_requests() {
    let mut fixed = Cursor::new(
        b"POST /api/rules/default HTTP/1.1\r\nHost: local\r\nContent-Length: 3\r\n\r\nabc".to_vec(),
    );
    let request = read_request(&mut fixed, 4096, 16, 4096).unwrap().unwrap();
    assert_eq!(request.body, b"abc");

    let mut chunked = Cursor::new(
        b"POST /api/rules/default HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n0\r\nX-End: yes\r\n\r\n"
            .to_vec(),
    );
    let request = read_request(&mut chunked, 4096, 16, 4096).unwrap().unwrap();
    assert_eq!(request.body, b"abc");
}

#[test]
fn rejects_ambiguous_framing_and_enforces_head_limits() {
    let mut ambiguous = Cursor::new(
        b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
    );
    assert!(read_request(&mut ambiguous, 4096, 16, 4096).is_err());

    let mut oversized = Cursor::new(b"GET / HTTP/1.1\r\nHost: local\r\n\r\n".to_vec());
    assert!(read_request(&mut oversized, 8, 16, 4096).is_err());

    let mut huge_declaration =
        Cursor::new(b"POST / HTTP/1.1\r\nContent-Length: 18446744073709551615\r\n\r\n".to_vec());
    assert!(read_request(&mut huge_declaration, 4096, 16, 1024).is_err());
}
