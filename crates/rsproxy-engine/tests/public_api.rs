//! Black-box checks for the supported `rsproxy-engine` facade.
//!
//! These tests deliberately import only public names and verify that runtime
//! composition, typed errors, CA injection, replay, and listener entry points
//! remain usable without reaching into engine implementation modules.

use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
};
use rsproxy_engine::{
    CaMaterial, EngineError, EngineHandle, EngineResult, IssuedLeafCertificate, ProxyConfig,
    ReplayResponse, RuleStore, RuleStoreError, SharedState, issue_leaf_certificate, serve,
};
use rsproxy_trace::Session;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STORAGE: AtomicU64 = AtomicU64::new(1);

#[test]
fn typed_error_facade_is_public() {
    fn assert_error<T: std::error::Error + Send + Sync + 'static>() {}
    fn accept_result(_: EngineResult<()>) {}
    fn convert_rule_store(error: RuleStoreError) -> EngineError {
        error.into()
    }

    assert_error::<EngineError>();
    assert_error::<RuleStoreError>();
    accept_result(Err(EngineError::Unsupported(
        "public API assertion".to_string(),
    )));
    let _convert: fn(RuleStoreError) -> EngineError = convert_rule_store;
}

#[test]
fn public_facade_builds_runtime_state_and_exposes_the_listener_entrypoint() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-engine-public-api-{}-{}",
        std::process::id(),
        NEXT_STORAGE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut config = ProxyConfig::new(&storage);
    config.trace_disk_budget = 0;

    let state = SharedState::new(config).unwrap();
    let handle = state.handle();
    assert_eq!(handle.status_snapshot().config.storage, storage);
    assert_eq!(handle.rules().snapshot().compiled.rules().len(), 0);
    assert_eq!(handle.trace_store().stats().sessions, 0);

    let _serve: fn(TcpListener, SharedState) -> EngineResult<()> = serve;
    let _state_new: fn(ProxyConfig) -> EngineResult<SharedState> = SharedState::new;
    let _replay: fn(&EngineHandle, &Session) -> EngineResult<ReplayResponse> = EngineHandle::replay;
    let _issue: fn(&str, &str, &str) -> EngineResult<IssuedLeafCertificate> =
        issue_leaf_certificate;

    drop(state);
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn rule_store_is_available_through_the_engine_facade() {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-engine-rule-store-api-{}-{}",
        std::process::id(),
        NEXT_STORAGE.fetch_add(1, Ordering::Relaxed)
    ));
    let store = RuleStore::load(&storage).unwrap();
    assert!(store.snapshot().group("default").is_some());
    let _ = std::fs::remove_dir_all(storage);
}

#[test]
fn leaf_issuance_is_available_without_platform_types() {
    let mut params = CertificateParams::default();
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, "rsproxy engine public API root");
    params.distinguished_name = name;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages.push(KeyUsagePurpose::KeyCertSign);
    params.key_usages.push(KeyUsagePurpose::DigitalSignature);
    let root_key = KeyPair::generate().unwrap();
    let root_certificate = params.self_signed(&root_key).unwrap();

    let issued = issue_leaf_certificate(
        &root_certificate.pem(),
        &root_key.serialize_pem(),
        "public-api.test",
    )
    .unwrap();
    assert!(issued.certificate_pem.contains("BEGIN CERTIFICATE"));
    assert!(issued.private_key_pem.contains("BEGIN PRIVATE KEY"));
    assert!(issued.chain_pem.ends_with(&root_certificate.pem()));
}

#[test]
fn ca_material_is_optional_cloneable_and_redacts_its_private_key() {
    assert!(ProxyConfig::default().ca_material.is_none());

    let private_key = "private-key-sentinel-that-must-not-leak";
    let material = CaMaterial::from_pem("certificate-pem", private_key);
    assert_eq!(material.clone().certificate_pem(), "certificate-pem");
    assert_eq!(material.private_key_pem(), private_key);

    let material_debug = format!("{material:?}");
    assert!(material_debug.contains("[REDACTED]"));
    assert!(!material_debug.contains(private_key));

    let config = ProxyConfig {
        ca_material: Some(material),
        ..ProxyConfig::default()
    };
    assert!(!format!("{config:?}").contains(private_key));
}
