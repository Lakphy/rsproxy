use super::*;

pub(super) fn parse_tls_op(args: &[&str]) -> Result<TlsOp, String> {
    if args.is_empty() {
        return Err("tls requires at least one option".to_string());
    }
    let mut client_cert = None;
    let mut client_key = None;
    let mut min_version = None;
    let mut ciphers = Vec::new();
    for arg in args {
        let (key, value) = arg
            .split_once('=')
            .ok_or_else(|| "tls options must use key=value syntax".to_string())?;
        let key = key.trim().to_ascii_lowercase().replace('_', "-");
        match key.as_str() {
            "client-cert" => {
                let value = normalize_tls_path(value);
                if value.is_empty() {
                    return Err("tls client-cert must not be empty".to_string());
                }
                client_cert = Some(value);
            }
            "client-key" => {
                let value = normalize_tls_path(value);
                if value.is_empty() {
                    return Err("tls client-key must not be empty".to_string());
                }
                client_key = Some(value);
            }
            "min" | "min-version" => {
                let value = unquote(value.trim());
                min_version = Some(parse_tls_min_version(&value)?);
            }
            "cipher" | "ciphers" => {
                let value = unquote(value.trim());
                if value.is_empty() {
                    return Err("tls ciphers must not be empty".to_string());
                }
                for name in value.split([':', '|', ';']) {
                    let cipher = parse_tls_cipher_suite(name)?;
                    if !ciphers.contains(&cipher) {
                        ciphers.push(cipher);
                    }
                }
            }
            _ => {
                return Err(format!(
                    "unsupported tls option `{key}`; supported: client-cert, client-key, min, ciphers"
                ));
            }
        }
    }
    if client_cert.is_some() != client_key.is_some() {
        return Err("tls client-cert and client-key must be configured together".to_string());
    }
    if matches!(min_version, Some(TlsMinVersion::Tls13))
        && !ciphers.is_empty()
        && ciphers.iter().all(|cipher| !cipher.is_tls13())
    {
        return Err("tls min=1.3 requires at least one TLS 1.3 cipher suite".to_string());
    }
    Ok(TlsOp {
        client_cert,
        client_key,
        min_version,
        ciphers,
    })
}

fn parse_tls_min_version(input: &str) -> Result<TlsMinVersion, String> {
    match input.trim().to_ascii_lowercase().as_str() {
        "1.2" | "tls1.2" | "tlsv1.2" | "tls12" | "tlsv12" => Ok(TlsMinVersion::Tls12),
        "1.3" | "tls1.3" | "tlsv1.3" | "tls13" | "tlsv13" => Ok(TlsMinVersion::Tls13),
        _ => Err(format!(
            "unsupported tls minimum version `{input}`; supported: 1.2, 1.3"
        )),
    }
}

fn parse_tls_cipher_suite(input: &str) -> Result<TlsCipherSuite, String> {
    let name = input.trim().to_ascii_uppercase();
    let cipher = match name.as_str() {
        "TLS_AES_128_GCM_SHA256" | "TLS13_AES_128_GCM_SHA256" => {
            TlsCipherSuite::Tls13Aes128GcmSha256
        }
        "TLS_AES_256_GCM_SHA384" | "TLS13_AES_256_GCM_SHA384" => {
            TlsCipherSuite::Tls13Aes256GcmSha384
        }
        "TLS_CHACHA20_POLY1305_SHA256" | "TLS13_CHACHA20_POLY1305_SHA256" => {
            TlsCipherSuite::Tls13Chacha20Poly1305Sha256
        }
        "TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256" | "ECDHE-ECDSA-AES128-GCM-SHA256" => {
            TlsCipherSuite::Tls12EcdheEcdsaAes128GcmSha256
        }
        "TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384" | "ECDHE-ECDSA-AES256-GCM-SHA384" => {
            TlsCipherSuite::Tls12EcdheEcdsaAes256GcmSha384
        }
        "TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256"
        | "ECDHE-ECDSA-CHACHA20-POLY1305"
        | "ECDHE-ECDSA-CHACHA20-POLY1305-SHA256" => {
            TlsCipherSuite::Tls12EcdheEcdsaChacha20Poly1305Sha256
        }
        "TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256" | "ECDHE-RSA-AES128-GCM-SHA256" => {
            TlsCipherSuite::Tls12EcdheRsaAes128GcmSha256
        }
        "TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384" | "ECDHE-RSA-AES256-GCM-SHA384" => {
            TlsCipherSuite::Tls12EcdheRsaAes256GcmSha384
        }
        "TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256"
        | "ECDHE-RSA-CHACHA20-POLY1305"
        | "ECDHE-RSA-CHACHA20-POLY1305-SHA256" => {
            TlsCipherSuite::Tls12EcdheRsaChacha20Poly1305Sha256
        }
        _ => return Err(format!("unsupported tls cipher suite `{input}`")),
    };
    Ok(cipher)
}

fn normalize_tls_path(input: &str) -> String {
    let value = unquote(input.trim());
    value
        .strip_prefix('<')
        .and_then(|path| path.strip_suffix('>'))
        .unwrap_or(&value)
        .to_string()
}
