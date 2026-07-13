use rsproxy_net::{RequestBodyFraming, read_request_head_tcp};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

#[test]
fn tcp_head_reader_preserves_body_and_pipelined_request_bytes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let first = read_request_head_tcp(&mut stream, 4096, 16)
            .unwrap()
            .unwrap();
        assert_eq!(first.request.target, "/upload");
        assert_eq!(first.body, RequestBodyFraming::ContentLength(4));
        let mut body = [0u8; 4];
        stream.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"body");

        let second = read_request_head_tcp(&mut stream, 4096, 16)
            .unwrap()
            .unwrap();
        assert_eq!(second.request.target, "/next");
        assert_eq!(second.body, RequestBodyFraming::None);
        assert!(
            read_request_head_tcp(&mut stream, 4096, 16)
                .unwrap()
                .is_none()
        );
    });

    let mut client = TcpStream::connect(address).unwrap();
    client
        .write_all(
            b"POST /upload HTTP/1.1\r\nHost: example.test\r\nContent-Length: 4\r\n\r\nbodyGET /next HTTP/1.1\r\nHost: example.test\r\n\r\n",
        )
        .unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    server.join().unwrap();
}

#[test]
fn tcp_head_reader_falls_back_for_fragmented_headers() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let head = read_request_head_tcp(&mut stream, 4096, 16)
            .unwrap()
            .unwrap();
        assert_eq!(head.request.method, "GET");
        assert_eq!(head.request.target, "/fragmented");
    });

    let mut client = TcpStream::connect(address).unwrap();
    client
        .write_all(b"GET /fragmented HTTP/1.1\r\nHost:")
        .unwrap();
    client.flush().unwrap();
    thread::sleep(Duration::from_millis(20));
    client.write_all(b" example.test\r\n\r\n").unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    server.join().unwrap();
}

#[test]
fn tcp_head_reader_keeps_header_limits_on_the_fast_path() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let error = read_request_head_tcp(&mut stream, 16, 16).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("header size limit exceeded"));
    });

    let mut client = TcpStream::connect(address).unwrap();
    client
        .write_all(b"GET / HTTP/1.1\r\nHost: example.test\r\n\r\n")
        .unwrap();
    client.shutdown(Shutdown::Write).unwrap();
    server.join().unwrap();
}
