use super::*;

pub(in crate::proxy) fn client_tls_record(
    phase: &str,
    host: &str,
    handshake_ms: u64,
    conn: &ClientConnection,
) -> TlsRecord {
    TlsRecord {
        phase: phase.to_string(),
        host: host.to_string(),
        handshake_ms,
        peer_certificates: conn
            .peer_certificates()
            .map(|certs| certs.len())
            .unwrap_or(0),
        protocol: protocol_name(conn.protocol_version()),
        cipher_suite: conn
            .negotiated_cipher_suite()
            .map(|suite| cipher_suite_name(suite.suite())),
        alpn: alpn_name(conn.alpn_protocol()),
        error: None,
    }
}

pub(in crate::proxy) fn server_tls_record(
    phase: &str,
    host: &str,
    handshake_ms: u64,
    conn: &ServerConnection,
) -> TlsRecord {
    TlsRecord {
        phase: phase.to_string(),
        host: host.to_string(),
        handshake_ms,
        peer_certificates: conn
            .peer_certificates()
            .map(|certs| certs.len())
            .unwrap_or(0),
        protocol: protocol_name(conn.protocol_version()),
        cipher_suite: conn
            .negotiated_cipher_suite()
            .map(|suite| cipher_suite_name(suite.suite())),
        alpn: alpn_name(conn.alpn_protocol()),
        error: None,
    }
}

pub(in crate::proxy) fn failed_tls_record(
    phase: &str,
    host: &str,
    started_ms: u64,
    err: &io::Error,
) -> TlsRecord {
    TlsRecord {
        phase: phase.to_string(),
        host: host.to_string(),
        handshake_ms: rsproxy_trace::now_millis().saturating_sub(started_ms),
        peer_certificates: 0,
        protocol: None,
        cipher_suite: None,
        alpn: None,
        error: Some(err.to_string()),
    }
}

fn protocol_name(version: Option<ProtocolVersion>) -> Option<String> {
    version.map(|version| format!("{version:?}"))
}

fn cipher_suite_name(cipher: CipherSuite) -> String {
    match cipher {
        CipherSuite::TLS13_AES_128_GCM_SHA256 => "TLS_AES_128_GCM_SHA256".to_string(),
        CipherSuite::TLS13_AES_256_GCM_SHA384 => "TLS_AES_256_GCM_SHA384".to_string(),
        CipherSuite::TLS13_CHACHA20_POLY1305_SHA256 => "TLS_CHACHA20_POLY1305_SHA256".to_string(),
        CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256 => {
            "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256".to_string()
        }
        CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384 => {
            "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384".to_string()
        }
        CipherSuite::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256 => {
            "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256".to_string()
        }
        CipherSuite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256 => {
            "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256".to_string()
        }
        CipherSuite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384 => {
            "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384".to_string()
        }
        CipherSuite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256 => {
            "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256".to_string()
        }
        _ => format!("{cipher:?}"),
    }
}

fn alpn_name(protocol: Option<&[u8]>) -> Option<String> {
    protocol.map(|value| String::from_utf8_lossy(value).into_owned())
}
