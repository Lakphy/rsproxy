use std::error::Error as _;

use super::*;

#[test]
fn certificate_parser_preserves_pem_order_and_skips_other_sections() {
    let pem = b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n\
                -----BEGIN PUBLIC KEY-----\nCQk=\n-----END PUBLIC KEY-----\n\
                -----BEGIN CERTIFICATE-----\nBAUG\n-----END CERTIFICATE-----\n";
    let certificates = parse_certs(pem).expect("parse certificate chain");
    assert_eq!(certificates.len(), 2);
    assert_eq!(certificates[0].as_ref(), &[1, 2, 3]);
    assert_eq!(certificates[1].as_ref(), &[4, 5, 6]);
}

#[test]
fn private_key_parser_uses_the_first_supported_key_section() {
    let pem = b"-----BEGIN PUBLIC KEY-----\nCQk=\n-----END PUBLIC KEY-----\n\
                -----BEGIN PRIVATE KEY-----\nMAA=\n-----END PRIVATE KEY-----\n\
                -----BEGIN RSA PRIVATE KEY-----\nMAE=\n-----END RSA PRIVATE KEY-----\n";
    let key = parse_private_key(pem).expect("parse first supported private key");
    assert!(matches!(key, PrivateKeyDer::Pkcs8(_)));
    assert_eq!(key.secret_der(), &[0x30, 0x00]);
}

#[test]
fn pem_errors_are_invalid_data_and_retain_the_parser_source() {
    let malformed = b"-----BEGIN CERTIFICATE-----\n%%%\n-----END CERTIFICATE-----\n";
    let error = parse_certs(malformed).expect_err("invalid base64 must fail");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.source().is_some());

    let missing =
        parse_private_key(b"-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n")
            .expect_err("missing private key must fail");
    assert_eq!(missing.kind(), io::ErrorKind::InvalidData);
    assert!(missing.source().is_some());
}

#[test]
fn pem_file_loader_rejects_one_byte_beyond_the_public_limit() {
    let path = std::env::temp_dir().join(format!(
        "rsproxy-pem-limit-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis()
    ));
    fs::write(&path, vec![b' '; rsproxy_rules::MAX_RULE_TLS_PEM_BYTES + 1]).unwrap();
    let error = load_certs(&path).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("1048576-byte limit"));
    let _ = fs::remove_file(path);
}
