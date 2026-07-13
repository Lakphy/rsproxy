use super::*;

fn finish_response<W: WsIo + Send>(
    client: &mut W,
    request: &RawRequest,
    request_meta: &RequestMeta,
    state: &SharedState,
    response: UpstreamH2Response,
) -> io::Result<ForwardResult> {
    let rules = state.rules.snapshot();
    finish_h2_response_with_context(
        client,
        ResponseContext {
            request,
            meta: request_meta,
            state,
            trace_id: 0,
            upstream_addr: "example.test:443".to_string(),
            client_connection: ClientPersistence::Close,
            deadline: request_deadline(),
        },
        &rules.compiled,
        response,
        false,
    )
}

#[test]
fn mitm_tls_configs_offer_supported_alpn() {
    let server = ServerConfig::builder()
        .with_no_client_auth()
        .with_cert_resolver(Arc::new(rustls::server::ResolvesServerCertUsingSni::new()));
    let server = with_mitm_server_alpn(server);
    assert_eq!(
        server.alpn_protocols,
        vec![H2_ALPN.to_vec(), HTTP1_ALPN.to_vec()]
    );

    let client = ClientConfig::builder()
        .with_root_certificates(RootCertStore::empty())
        .with_no_client_auth();
    let proxy_client = with_client_alpn(client.clone(), false);
    assert_eq!(proxy_client.alpn_protocols, vec![HTTP1_ALPN.to_vec()]);

    let origin_client = with_client_alpn(client, true);
    assert_eq!(
        origin_client.alpn_protocols,
        vec![H2_ALPN.to_vec(), HTTP1_ALPN.to_vec()]
    );
}

#[test]
fn upstream_root_cache_merges_webpki_and_native_certificates() {
    let key = KeyPair::generate().unwrap();
    let cert = CertificateParams::new(vec!["native-root.test".to_string()])
        .unwrap()
        .self_signed(&key)
        .unwrap();

    let cache = build_upstream_root_cache(
        vec![
            cert.der().clone(),
            cert.der().clone(),
            CertificateDer::from(vec![0x01, 0x02, 0x03]),
        ],
        vec!["fixture warning".to_string()],
    );

    assert_eq!(cache.webpki_roots, webpki_roots::TLS_SERVER_ROOTS.len());
    assert_eq!(cache.native_loaded, 2);
    assert_eq!(cache.native_rejected, 1);
    assert_eq!(cache.native_duplicates, 1);
    assert_eq!(cache.native_errors, vec!["fixture warning".to_string()]);
    assert_eq!(cache.total_roots, cache.webpki_roots + 1);
    assert_eq!(cache.roots.len(), cache.total_roots);
}

#[test]
fn upstream_tls_policy_filters_crypto_provider_ciphers() {
    let policy = TlsOp {
        client_cert: None,
        client_key: None,
        min_version: Some(TlsMinVersion::Tls13),
        ciphers: vec![
            TlsCipherSuite::Tls13Aes128GcmSha256,
            TlsCipherSuite::Tls13Aes256GcmSha384,
        ],
    };

    let config = mitm_client_config(&test_state(), None, Some(&policy), true).unwrap();
    let suites = config
        .crypto_provider()
        .cipher_suites
        .iter()
        .map(|suite| suite.suite())
        .collect::<Vec<_>>();

    assert_eq!(
        suites,
        vec![
            CipherSuite::TLS13_AES_128_GCM_SHA256,
            CipherSuite::TLS13_AES_256_GCM_SHA384,
        ]
    );
    assert_eq!(
        config.alpn_protocols,
        vec![H2_ALPN.to_vec(), HTTP1_ALPN.to_vec()]
    );
}

#[test]
fn h2_bridge_reuses_rule_and_trace_pipeline() {
    assert_eq!(upstream_http_version("HTTP/2"), "HTTP/1.1");
    assert_eq!(upstream_http_version("HTTP/1.0"), "HTTP/1.0");
    let mut response_headers = vec![
        ("Connection".to_string(), "keep-alive, x-hop".to_string()),
        ("Keep-Alive".to_string(), "timeout=120".to_string()),
        ("X-Hop".to_string(), "remove".to_string()),
        ("X-End-To-End".to_string(), "keep".to_string()),
        ("Transfer-Encoding".to_string(), "chunked".to_string()),
    ];
    prepare_h2_client_response_headers(&mut response_headers, 200, Some(7));
    assert!(http::header(&response_headers, "transfer-encoding").is_none());
    assert!(http::header(&response_headers, "connection").is_none());
    assert!(http::header(&response_headers, "keep-alive").is_none());
    assert!(http::header(&response_headers, "x-hop").is_none());
    assert_eq!(
        http::header(&response_headers, "x-end-to-end"),
        Some("keep")
    );
    assert_eq!(http::header(&response_headers, "content-length"), Some("7"));
    let mut state = test_state();
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        rsproxy_rules::RuleSet::parse("default", "example.test status(218) tag(h2:${path})")
            .unwrap(),
    );
    let request = RawRequest {
        method: "POST".to_string(),
        target: "/bridge?x=1".to_string(),
        version: "HTTP/2".to_string(),
        headers: vec![("Host".to_string(), "example.test".to_string())],
        body: b"payload".to_vec(),
        trailers: vec![("x-request-checksum".to_string(), "abc".to_string())],
    };
    let client_tls = TlsRecord {
        phase: "client_mitm_tls".to_string(),
        host: "example.test".to_string(),
        handshake_ms: 3,
        peer_certificates: 0,
        protocol: Some("TLSv1_3".to_string()),
        cipher_suite: Some("TLS_AES_128_GCM_SHA256".to_string()),
        alpn: Some("h2".to_string()),
        error: None,
    };

    let (response, frames) = process_h2_request_collected(
        request,
        state.clone(),
        "127.0.0.1:12345".to_string(),
        "example.test".to_string(),
        client_tls,
        vec!["h2-client".to_string()],
    )
    .unwrap();

    assert_eq!(response.status, 218);
    let body = frames
        .iter()
        .filter_map(|frame| match frame {
            DownstreamH2ResponseFrame::Data(data) => Some(data.as_ref()),
            DownstreamH2ResponseFrame::Trailers(_) => None,
        })
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    assert!(String::from_utf8_lossy(&body).contains("status(218)"));
    let sessions = state.trace.list(1);
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].url, "https://example.test/bridge?x=1");
    assert!(sessions[0].flags.contains(&"h2-client".to_string()));
    assert!(sessions[0].flags.contains(&"req-trailers".to_string()));
    assert!(sessions[0].flags.contains(&"tag:h2:/bridge".to_string()));
    assert_eq!(
        sessions[0].req_trailers,
        vec![("x-request-checksum".to_string(), "abc".to_string())]
    );
    assert_eq!(sessions[0].tls[0].alpn.as_deref(), Some("h2"));
}

#[test]
fn upstream_h2_response_reuses_response_rules_and_preserves_grpc_trailers() {
    let mut state = test_state();
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        rsproxy_rules::RuleSet::parse(
            "default",
            "example.test res.header(x-h2-rule: yes) res.trailer(x-rule-trailer: done) when status(200)",
        )
        .unwrap(),
    );
    let request = RawRequest {
        method: "POST".to_string(),
        target: "/grpc.Echo/Call".to_string(),
        version: "HTTP/1.1".to_string(),
        headers: vec![("Content-Type".to_string(), "application/grpc".to_string())],
        body: vec![0, 0, 0, 0, 0],
        trailers: Vec::new(),
    };
    let request_meta = RequestMeta {
        method: request.method.clone(),
        url: "https://example.test/grpc.Echo/Call".to_string(),
        headers: request.headers.clone(),
        body: request.body.clone(),
        client_ip: Some("127.0.0.1:12345".to_string()),
        server_ip: None,
        template: Default::default(),
    };
    let response = UpstreamH2Response {
        status: 200,
        headers: vec![("content-type".to_string(), "application/grpc".to_string())],
        body: UpstreamBody::from_collected(
            request.body.clone(),
            vec![("grpc-status".to_string(), "0".to_string())],
        ),
        reused_connection: true,
        pool_wait_ms: 4,
        request_send_ms: 0,
        ttfb_ms: 2,
    };
    let mut capture = CapturedHttpResponse::default();

    let result = finish_response(&mut capture, &request, &request_meta, &state, response).unwrap();

    assert_eq!(
        result.protocol,
        UpstreamProtocol::Http2 {
            reused_connection: true
        }
    );
    assert_eq!(result.pool_wait_ms, 4);
    assert_eq!(http::header(&result.res_headers, "x-h2-rule"), Some("yes"));
    assert_eq!(http::header(&result.res_trailers, "grpc-status"), Some("0"));
    assert_eq!(
        http::header(&result.res_trailers, "x-rule-trailer"),
        Some("done")
    );
    let captured = String::from_utf8_lossy(&capture.bytes);
    assert!(captured.starts_with("HTTP/1.1 200 OK\r\n"));
    assert!(captured.contains("Transfer-Encoding: chunked\r\n"));
    assert!(captured.ends_with("grpc-status: 0\r\nX-Rule-Trailer: done\r\n\r\n"));
}

#[test]
fn http10_buffered_response_uses_length_and_suppresses_trailers() {
    let mut state = test_state();
    state.rules = RuleStore::from_compiled(
        &state.config.storage,
        rsproxy_rules::RuleSet::parse(
            "default",
            "example.test res.trailer(x-rule-trailer: legacy)",
        )
        .unwrap(),
    );
    let request = RawRequest {
        method: "GET".to_string(),
        target: "/legacy".to_string(),
        version: "HTTP/1.0".to_string(),
        headers: vec![("Host".to_string(), "example.test".to_string())],
        body: Vec::new(),
        trailers: Vec::new(),
    };
    let request_meta = meta("https://example.test/legacy");
    let response = UpstreamH2Response {
        status: 200,
        headers: vec![("content-type".to_string(), "text/plain".to_string())],
        body: UpstreamBody::from_collected(
            b"legacy".to_vec(),
            vec![("x-origin-trailer".to_string(), "legacy".to_string())],
        ),
        reused_connection: false,
        pool_wait_ms: 0,
        request_send_ms: 0,
        ttfb_ms: 1,
    };
    let mut capture = CapturedHttpResponse::default();

    let result = finish_response(&mut capture, &request, &request_meta, &state, response).unwrap();

    assert!(result.res_trailers.is_empty());
    assert_eq!(
        http::header(&result.res_headers, "content-length"),
        Some("6")
    );
    assert!(http::header(&result.res_headers, "transfer-encoding").is_none());
    assert!(http::header(&result.res_headers, "trailer").is_none());
    let captured = String::from_utf8(capture.bytes).unwrap();
    assert!(captured.starts_with("HTTP/1.0 200 OK\r\n"));
    assert!(captured.ends_with("legacy"));
}
