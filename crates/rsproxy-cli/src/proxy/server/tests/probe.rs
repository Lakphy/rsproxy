use super::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

fn tcp_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
    let (server, _) = listener.accept().unwrap();
    (client, server)
}

#[test]
fn prefix_classifier_separates_tls_http_and_unknown_protocols() {
    assert_eq!(classify_prefix(&[0x16]), PrefixState::NeedMore);
    assert_eq!(
        classify_prefix(&[0x16, 0x03, 0x01, 0x00]),
        PrefixState::Protocol(ConnectProtocol::Tls)
    );
    assert_eq!(classify_prefix(b"GET "), PrefixState::NeedMore);
    assert_eq!(
        classify_prefix(b"GET /resource HTTP/1.1\r\n"),
        PrefixState::Protocol(ConnectProtocol::Http)
    );
    assert_eq!(
        classify_prefix(b"CUSTOM https://example.test/ HTTP/1.1\r\n"),
        PrefixState::Protocol(ConnectProtocol::Http)
    );
    assert_eq!(
        classify_prefix(b"EHLO mail.example\r\n"),
        PrefixState::Protocol(ConnectProtocol::Unknown)
    );
    assert_eq!(
        classify_prefix(&[0x01, 0x02, 0x03]),
        PrefixState::Protocol(ConnectProtocol::Unknown)
    );
}

#[test]
fn detection_peeks_without_consuming_client_bytes() {
    let (mut client, mut server) = tcp_pair();
    client.write_all(b"GET /inside HTTP/1.1\r\n").unwrap();

    assert_eq!(
        detect(&mut server, Duration::from_millis(100)).unwrap(),
        ConnectProtocol::Http
    );
    let mut observed = [0u8; 4];
    server.read_exact(&mut observed).unwrap();
    assert_eq!(&observed, b"GET ");
}

#[test]
fn detection_timeout_restores_the_previous_socket_timeout() {
    let (_client, mut server) = tcp_pair();
    let original = Duration::from_secs(2);
    server.set_read_timeout(Some(original)).unwrap();

    assert_eq!(
        detect(&mut server, Duration::from_millis(20)).unwrap(),
        ConnectProtocol::Timeout
    );
    assert_eq!(server.read_timeout().unwrap(), Some(original));
}
