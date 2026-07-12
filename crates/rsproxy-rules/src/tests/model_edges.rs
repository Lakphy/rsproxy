use super::*;

#[test]
fn public_value_and_pattern_helpers_preserve_source_and_identity() {
    let inline = Value::inline("text");
    let file = Value::File("body.bin".to_string());
    let reference = Value::Reference("payload".to_string());
    assert_eq!(inline.as_inline(), Some("text"));
    assert_eq!(file.as_inline(), None);
    assert_eq!(reference.as_inline(), None);
    assert_eq!(inline.source(), "text");
    assert_eq!(file.source(), "body.bin");
    assert_eq!(reference.source(), "payload");

    let sensitive = RegexReplacePattern::new("token".to_string(), false).unwrap();
    let insensitive = RegexReplacePattern::new("token".to_string(), true).unwrap();
    assert_eq!(sensitive.replace_all("token TOKEN", "x"), "x TOKEN");
    assert_eq!(insensitive.replace_all("token TOKEN", "x"), "x x");
    assert_ne!(sensitive, insensitive);
    assert_eq!(sensitive, sensitive.clone());
    assert_eq!(
        format!("{sensitive:?}"),
        "RegexReplacePattern { pattern: \"token\", case_insensitive: false }"
    );
}

#[test]
fn host_pool_clones_share_rotation_but_keep_one_selection_per_request() {
    let pool = HostPool::new(vec![Value::inline("a"), Value::inline("b")]).unwrap();
    let first_request = pool.clone();
    let second_request = pool.clone();
    assert_eq!(first_request.selected_address().source(), "a");
    assert_eq!(first_request.selected_address().source(), "a");
    assert_eq!(second_request.selected_address().source(), "b");
    assert_eq!(pool, pool.clone());
    assert_eq!(
        format!("{pool:?}"),
        "HostPool { addresses: [Inline(\"a\"), Inline(\"b\")], .. }"
    );
}

#[test]
fn compiled_index_and_regex_equality_ignore_runtime_matcher_state() {
    let first = RuleSet::parse(
        "default",
        "/example\\.test/ status(200)\nexample.test status(201)",
    )
    .unwrap();
    let second = first.clone();
    assert_eq!(first.index, second.index);
    assert_eq!(first, second);

    let Action::Status(_) = first.rules[0].actions[0] else {
        panic!("status action expected");
    };
    let Matcher::Regex(left) = &first.rules[0].matcher else {
        panic!("regex matcher expected");
    };
    let Matcher::Regex(right) = &second.rules[0].matcher else {
        panic!("regex matcher expected");
    };
    assert_eq!(left, right);

    let mut changed = second.index.clone();
    changed.global.push(99);
    assert_ne!(first.index, changed);
}

#[test]
fn cipher_metadata_covers_every_supported_suite() {
    let suites = [
        (
            TlsCipherSuite::Tls13Aes128GcmSha256,
            "TLS_AES_128_GCM_SHA256",
            true,
        ),
        (
            TlsCipherSuite::Tls13Aes256GcmSha384,
            "TLS_AES_256_GCM_SHA384",
            true,
        ),
        (
            TlsCipherSuite::Tls13Chacha20Poly1305Sha256,
            "TLS_CHACHA20_POLY1305_SHA256",
            true,
        ),
        (
            TlsCipherSuite::Tls12EcdheEcdsaAes128GcmSha256,
            "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256",
            false,
        ),
        (
            TlsCipherSuite::Tls12EcdheEcdsaAes256GcmSha384,
            "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384",
            false,
        ),
        (
            TlsCipherSuite::Tls12EcdheEcdsaChacha20Poly1305Sha256,
            "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256",
            false,
        ),
        (
            TlsCipherSuite::Tls12EcdheRsaAes128GcmSha256,
            "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256",
            false,
        ),
        (
            TlsCipherSuite::Tls12EcdheRsaAes256GcmSha384,
            "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384",
            false,
        ),
        (
            TlsCipherSuite::Tls12EcdheRsaChacha20Poly1305Sha256,
            "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256",
            false,
        ),
    ];
    for (suite, name, tls13) in suites {
        assert_eq!(suite.as_str(), name);
        assert_eq!(suite.is_tls13(), tls13);
    }
    assert_eq!(TlsMinVersion::Tls12.as_str(), "1.2");
    assert_eq!(TlsMinVersion::Tls13.as_str(), "1.3");
}
