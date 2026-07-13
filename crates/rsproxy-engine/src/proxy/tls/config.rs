use super::*;

pub(in crate::proxy) fn ca_initialized(state: &SharedState) -> bool {
    state.config.ca_material.is_some()
}

pub(in crate::proxy) fn mitm_server_config(
    state: &SharedState,
    host: &str,
) -> io::Result<(Arc<ServerConfig>, bool)> {
    if let Some(config) = state
        .mitm_cert_cache
        .lock()
        .expect("MITM certificate cache lock poisoned")
        .get(host)
    {
        return Ok((config, true));
    }
    let ca_material = state.config.ca_material.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "root CA material was not injected")
    })?;
    let (cert_path, key_path) =
        ensure_leaf_certificate(&state.config.storage.join("ca"), ca_material, host)?;
    let certs = load_certs(&cert_path)?;
    let key = load_private_key(&key_path)?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let config = Arc::new(with_mitm_server_alpn(config));
    state
        .mitm_cert_cache
        .lock()
        .expect("MITM certificate cache lock poisoned")
        .insert(host.to_string(), Arc::clone(&config));
    Ok((config, false))
}

pub(in crate::proxy) fn mitm_client_config(
    state: &SharedState,
    client_identity: Option<TlsClientIdentity>,
    tls_policy: Option<&TlsOp>,
    allow_h2: bool,
) -> io::Result<ClientConfig> {
    let roots = mitm_root_store(state)?;
    let mut provider = rustls::crypto::aws_lc_rs::default_provider();
    if let Some(op) = tls_policy.filter(|op| !op.ciphers.is_empty()) {
        let available = provider.cipher_suites.clone();
        provider.cipher_suites = op
            .ciphers
            .iter()
            .filter_map(|cipher| {
                let wanted = tls_cipher_suite_id(*cipher);
                available
                    .iter()
                    .find(|suite| suite.suite() == wanted)
                    .copied()
            })
            .collect();
        if provider.cipher_suites.len() != op.ciphers.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "one or more configured TLS cipher suites are unavailable",
            ));
        }
    }
    let versions = match tls_policy.and_then(|op| op.min_version) {
        Some(TlsMinVersion::Tls13) => vec![&rustls::version::TLS13],
        _ => vec![&rustls::version::TLS13, &rustls::version::TLS12],
    };
    let builder = ClientConfig::builder_with_provider(Arc::new(provider))
        .with_protocol_versions(&versions)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))?
        .with_root_certificates(roots);
    let config = match client_identity {
        Some(identity) => builder
            .with_client_auth_cert(identity.certs, identity.key)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?,
        None => builder.with_no_client_auth(),
    };
    Ok(with_client_alpn(config, allow_h2))
}

fn tls_cipher_suite_id(cipher: TlsCipherSuite) -> CipherSuite {
    match cipher {
        TlsCipherSuite::Tls13Aes128GcmSha256 => CipherSuite::TLS13_AES_128_GCM_SHA256,
        TlsCipherSuite::Tls13Aes256GcmSha384 => CipherSuite::TLS13_AES_256_GCM_SHA384,
        TlsCipherSuite::Tls13Chacha20Poly1305Sha256 => CipherSuite::TLS13_CHACHA20_POLY1305_SHA256,
        TlsCipherSuite::Tls12EcdheEcdsaAes128GcmSha256 => {
            CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_128_GCM_SHA256
        }
        TlsCipherSuite::Tls12EcdheEcdsaAes256GcmSha384 => {
            CipherSuite::TLS_ECDHE_ECDSA_WITH_AES_256_GCM_SHA384
        }
        TlsCipherSuite::Tls12EcdheEcdsaChacha20Poly1305Sha256 => {
            CipherSuite::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
        }
        TlsCipherSuite::Tls12EcdheRsaAes128GcmSha256 => {
            CipherSuite::TLS_ECDHE_RSA_WITH_AES_128_GCM_SHA256
        }
        TlsCipherSuite::Tls12EcdheRsaAes256GcmSha384 => {
            CipherSuite::TLS_ECDHE_RSA_WITH_AES_256_GCM_SHA384
        }
        TlsCipherSuite::Tls12EcdheRsaChacha20Poly1305Sha256 => {
            CipherSuite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
        }
    }
}

pub(in crate::proxy) fn tls_client_identity(
    actions: &[ResolvedAction],
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<Option<TlsClientIdentity>> {
    let Some(item) = tls_action(actions) else {
        return Ok(None);
    };
    let op = tls_action_op(item);
    let (Some(client_cert), Some(client_key)) = (&op.client_cert, &op.client_key) else {
        return Ok(None);
    };
    let cert_path = resolve_tls_file_path(&item.render(client_cert, meta), state);
    let key_path = resolve_tls_file_path(&item.render(client_key, meta), state);
    let certs = load_certs(&cert_path).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "failed to load tls client-cert {}: {err}",
                cert_path.display()
            ),
        )
    })?;
    if certs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "tls client-cert {} contains no certificates",
                cert_path.display()
            ),
        ));
    }
    let key = load_private_key(&key_path).map_err(|err| {
        io::Error::new(
            err.kind(),
            format!(
                "failed to load tls client-key {}: {err}",
                key_path.display()
            ),
        )
    })?;
    Ok(Some(TlsClientIdentity { certs, key }))
}

pub(in crate::proxy) fn resolve_tls_file_path(path: &str, state: &SharedState) -> PathBuf {
    let raw = PathBuf::from(path);
    if raw.is_absolute() {
        return raw;
    }
    let storage_path = state.config.storage.join(&raw);
    if storage_path.exists() {
        storage_path
    } else {
        raw
    }
}

pub(in crate::proxy) fn with_mitm_server_alpn(mut config: ServerConfig) -> ServerConfig {
    config.alpn_protocols = vec![H2_ALPN.to_vec(), HTTP1_ALPN.to_vec()];
    config
}

pub(in crate::proxy) fn with_client_alpn(mut config: ClientConfig, allow_h2: bool) -> ClientConfig {
    config.alpn_protocols = if allow_h2 {
        vec![H2_ALPN.to_vec(), HTTP1_ALPN.to_vec()]
    } else {
        http1_alpn_protocols()
    };
    config
}

fn http1_alpn_protocols() -> Vec<Vec<u8>> {
    vec![HTTP1_ALPN.to_vec()]
}

pub(crate) fn initialize_upstream_roots(state: &SharedState) -> &UpstreamRootCache {
    state.upstream_roots.get_or_init(|| {
        let native = rustls_native_certs::load_native_certs();
        let errors = native
            .errors
            .into_iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>();
        let cache = build_upstream_root_cache(native.certs, errors);
        tracing::info!(
            event = "upstream_trust_roots_loaded",
            webpki = cache.webpki_roots,
            native_loaded = cache.native_loaded,
            native_rejected = cache.native_rejected,
            native_duplicates = cache.native_duplicates,
            total = cache.total_roots,
            native_errors = cache.native_errors.len(),
            "upstream trust roots loaded"
        );
        for error in cache.native_errors.iter().take(3) {
            tracing::warn!(
                event = "native_trust_root_rejected",
                error = %error,
                "native trust root rejected"
            );
        }
        cache
    })
}

pub(in crate::proxy) fn build_upstream_root_cache(
    native_certs: Vec<CertificateDer<'static>>,
    native_errors: Vec<String>,
) -> UpstreamRootCache {
    let mut roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let webpki_roots = roots.len();
    let (native_loaded, native_rejected) = roots.add_parsable_certificates(native_certs);
    let roots_before_dedup = roots.len();
    let mut unique = Vec::with_capacity(roots_before_dedup);
    for anchor in std::mem::take(&mut roots.roots) {
        if !unique.contains(&anchor) {
            unique.push(anchor);
        }
    }
    roots.roots = unique;
    let native_duplicates = roots_before_dedup.saturating_sub(roots.len());
    let total_roots = roots.len();
    UpstreamRootCache {
        roots,
        webpki_roots,
        native_loaded,
        native_rejected,
        native_duplicates,
        total_roots,
        native_errors,
    }
}

fn mitm_root_store(state: &SharedState) -> io::Result<RootCertStore> {
    let mut roots = initialize_upstream_roots(state).roots.clone();
    if let Some(ca_material) = &state.config.ca_material {
        for cert in load_certs_from_pem(ca_material.certificate_pem())? {
            roots
                .add(cert)
                .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        }
    }
    Ok(roots)
}
