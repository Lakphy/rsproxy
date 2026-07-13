use super::*;
use std::io::Cursor;

#[derive(Default)]
struct ScriptedIo {
    input: Cursor<Vec<u8>>,
    output: Vec<u8>,
}

impl ScriptedIo {
    fn new(input: impl Into<Vec<u8>>) -> Self {
        Self {
            input: Cursor::new(input.into()),
            output: Vec::new(),
        }
    }
}

impl Read for ScriptedIo {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.input.read(buf)
    }
}

impl Write for ScriptedIo {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.output.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn socks_response(atyp: u8, address: &[u8], reply: u8) -> Vec<u8> {
    let mut response = vec![0x05, 0x00, 0x05, reply, 0x00, atyp];
    if atyp == 0x03 {
        response.push(address.len() as u8);
    }
    response.extend_from_slice(address);
    response.extend_from_slice(&8080u16.to_be_bytes());
    response
}

#[test]
fn http_connect_accepts_success_and_rejects_proxy_failure() {
    let mut success = ScriptedIo::new(b"HTTP/1.1 204 No Content\r\nX-Proxy: test\r\n\r\n".to_vec());
    http_proxy_connect_tunnel(&mut success, "origin.test:443", 4096, 16).unwrap();
    assert_eq!(
        success.output,
        b"CONNECT origin.test:443 HTTP/1.1\r\nHost: origin.test:443\r\nConnection: close\r\n\r\n"
    );

    let mut rejected =
        ScriptedIo::new(b"HTTP/1.1 407 Proxy Authentication Required\r\n\r\n".to_vec());
    let error = http_proxy_connect_tunnel(&mut rejected, "origin.test:443", 4096, 16).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(
        error
            .to_string()
            .contains("407 Proxy Authentication Required")
    );
}

#[test]
fn socks5_connect_encodes_domain_ipv4_and_ipv6_targets() {
    let cases = [
        (
            "origin.test",
            0x03,
            vec![
                11, b'o', b'r', b'i', b'g', b'i', b'n', b'.', b't', b'e', b's', b't',
            ],
            socks_response(0x03, b"proxy", 0),
        ),
        (
            "127.0.0.1",
            0x01,
            vec![127, 0, 0, 1],
            socks_response(0x01, &[127, 0, 0, 1], 0),
        ),
        (
            "2001:db8::1",
            0x04,
            "2001:db8::1"
                .parse::<std::net::Ipv6Addr>()
                .unwrap()
                .octets()
                .to_vec(),
            socks_response(0x04, &[0; 16], 0),
        ),
    ];

    for (host, atyp, encoded_host, response) in cases {
        let mut io = ScriptedIo::new(response);
        socks5_connect(&mut io, host, 443, None).unwrap();

        let mut expected = vec![0x05, 0x01, 0x00, 0x05, 0x01, 0x00, atyp];
        expected.extend_from_slice(&encoded_host);
        expected.extend_from_slice(&443u16.to_be_bytes());
        assert_eq!(io.output, expected, "target={host}");
    }
}

#[test]
fn socks5_connect_performs_username_password_authentication() {
    let auth = SocksAuth {
        username: "alice".to_string(),
        password: "secret".to_string(),
    };
    let mut input = vec![0x05, 0x02, 0x01, 0x00];
    input.extend(socks_response(0x01, &[0, 0, 0, 0], 0).into_iter().skip(2));
    let mut io = ScriptedIo::new(input);

    socks5_connect(&mut io, "10.0.0.1", 8443, Some(&auth)).unwrap();

    let mut expected = vec![0x05, 0x02, 0x00, 0x02, 0x01, 5];
    expected.extend_from_slice(b"alice");
    expected.push(6);
    expected.extend_from_slice(b"secret");
    expected.extend_from_slice(&[0x05, 0x01, 0x00, 0x01, 10, 0, 0, 1]);
    expected.extend_from_slice(&8443u16.to_be_bytes());
    assert_eq!(io.output, expected);
}

#[test]
fn socks5_greeting_and_target_validation_errors_are_specific() {
    let mut invalid_version = ScriptedIo::new(vec![0x04, 0x00]);
    let error = socks5_connect(&mut invalid_version, "host", 80, None).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);

    let mut missing_auth = ScriptedIo::new(vec![0x05, 0x02]);
    let error = socks5_connect(&mut missing_auth, "host", 80, None).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("no credentials"));

    let mut unsupported = ScriptedIo::new(vec![0x05, 0xff]);
    let error = socks5_connect(&mut unsupported, "host", 80, None).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    assert!(error.to_string().contains("0xff"));

    let mut too_long = ScriptedIo::new(vec![0x05, 0x00]);
    let error = socks5_connect(&mut too_long, &"x".repeat(256), 80, None).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}

#[test]
fn socks5_connect_rejects_malformed_and_failed_responses() {
    let cases = [
        (
            vec![0x05, 0x00, 0x04, 0x00, 0x00, 0x01],
            "invalid SOCKS5 connect response",
        ),
        (
            vec![0x05, 0x00, 0x05, 0x00, 0x00, 0x7f],
            "invalid SOCKS5 address type",
        ),
        (socks_response(0x01, &[0, 0, 0, 0], 0x05), "reply 0x05"),
    ];

    for (input, message) in cases {
        let mut io = ScriptedIo::new(input);
        let error = socks5_connect(&mut io, "host", 80, None).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
}

#[test]
fn socks5_auth_rejects_oversized_credentials_and_bad_responses() {
    let oversized = SocksAuth {
        username: "u".repeat(256),
        password: String::new(),
    };
    let error = socks5_username_password_auth(&mut ScriptedIo::default(), &oversized).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);

    let auth = SocksAuth {
        username: "u".to_string(),
        password: "p".to_string(),
    };
    for (response, kind, message) in [
        ([0x02, 0x00], io::ErrorKind::InvalidData, "invalid"),
        ([0x01, 0x01], io::ErrorKind::PermissionDenied, "failed"),
    ] {
        let mut io = ScriptedIo::new(response.to_vec());
        let error = socks5_username_password_auth(&mut io, &auth).unwrap_err();
        assert_eq!(error.kind(), kind);
        assert!(error.to_string().contains(message));
    }
}

#[test]
fn discard_and_stage_helpers_preserve_error_semantics() {
    let bytes = (0..70).collect::<Vec<u8>>();
    let mut input = bytes.as_slice();
    read_exact_discard(&mut input, 65).unwrap();
    assert_eq!(input, &[65, 66, 67, 68, 69]);

    let mut short = [1u8, 2].as_slice();
    assert_eq!(
        read_exact_discard(&mut short, 3).unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );

    let error = stage_io_error(
        "proxy_connect",
        io::Error::new(io::ErrorKind::ConnectionRefused, "refused"),
    );
    assert_eq!(error.kind(), io::ErrorKind::ConnectionRefused);
    assert_eq!(error.to_string(), "stage=proxy_connect: refused");
}
