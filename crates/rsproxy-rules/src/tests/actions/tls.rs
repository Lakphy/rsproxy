use super::super::*;

#[test]
fn parses_tls_client_cert_action_and_explains_templates() {
    let rules = RuleSet::parse(
        "default",
        "example.com tls(client-cert=<certs/${host}.pem>, client_key=\"keys/${host}.key\")",
    )
    .unwrap();
    assert!(matches!(
        &rules.rules()[0].actions[0],
        Action::Tls(TlsOp { client_cert: Some(client_cert), client_key: Some(client_key), .. })
            if client_cert == "certs/${host}.pem" && client_key == "keys/${host}.key"
    ));

    assert_eq!(
        rules.explain(&req("http://example.com/")),
        "default:1 tls(client-cert=certs/example.com.pem, client-key=keys/example.com.key)\n"
    );
}

#[test]
fn parses_tls_version_and_cipher_policy_with_aliases() {
    let rules = RuleSet::parse(
            "default",
            "example.com tls(min=TLSv1.2, ciphers=ECDHE-ECDSA-AES128-GCM-SHA256:TLS_AES_128_GCM_SHA256)",
        )
        .unwrap();
    assert!(matches!(
        &rules.rules()[0].actions[0],
        Action::Tls(TlsOp {
            client_cert: None,
            client_key: None,
            min_version: Some(TlsMinVersion::Tls12),
            ciphers,
        }) if ciphers == &vec![
            TlsCipherSuite::Tls12EcdheEcdsaAes128GcmSha256,
            TlsCipherSuite::Tls13Aes128GcmSha256,
        ]
    ));
    assert_eq!(
        rules.explain(&req("https://example.com/")),
        "default:1 tls(min=1.2, ciphers=TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256:TLS_AES_128_GCM_SHA256)\n"
    );
}

#[test]
fn tls_action_rejects_invalid_options_or_missing_key_pair() {
    let missing = RuleSet::parse("default", "example.com tls(client-cert=client.pem)").unwrap_err();
    assert_eq!(
        missing[0].message,
        "tls client-cert and client-key must be configured together"
    );

    let unsupported = RuleSet::parse(
        "default",
        "example.com tls(max=1.3, client-cert=a, client-key=b)",
    )
    .unwrap_err();
    assert_eq!(
        unsupported[0].message,
        "unsupported tls option `max`; supported: client-cert, client-key, min, ciphers"
    );

    let invalid_version = RuleSet::parse("default", "example.com tls(min=1.1)").unwrap_err();
    assert_eq!(
        invalid_version[0].message,
        "unsupported tls minimum version `1.1`; supported: 1.2, 1.3"
    );

    let invalid_cipher = RuleSet::parse(
        "default",
        "example.com tls(ciphers=TLS_RSA_WITH_3DES_EDE_CBC_SHA)",
    )
    .unwrap_err();
    assert_eq!(
        invalid_cipher[0].message,
        "unsupported tls cipher suite `TLS_RSA_WITH_3DES_EDE_CBC_SHA`"
    );

    let incompatible = RuleSet::parse(
        "default",
        "example.com tls(min=1.3, ciphers=ECDHE-RSA-AES128-GCM-SHA256)",
    )
    .unwrap_err();
    assert_eq!(
        incompatible[0].message,
        "tls min=1.3 requires at least one TLS 1.3 cipher suite"
    );

    for source in [
        "example.com tls(client-cert=bad\0cert, client-key=key.pem)",
        "example.com tls(client-cert=cert.pem, client-key=bad\0key)",
    ] {
        let invalid_path = RuleSet::parse("default", source).unwrap_err();
        assert!(invalid_path[0].message.contains("must not contain NUL"));
    }
}
