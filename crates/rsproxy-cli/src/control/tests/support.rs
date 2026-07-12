use crate::app::{AppConfig, MitmCertCache, MitmFailureCache, SharedState};
use crate::http::RawRequest;
use crate::rule_store::RuleStore;
use rsproxy_rules::RuleSet;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

static NEXT_STORAGE: AtomicU64 = AtomicU64::new(1);

pub(super) fn test_state() -> SharedState {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-control-routes-{}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis(),
        NEXT_STORAGE.fetch_add(1, Ordering::Relaxed)
    ));
    let config = AppConfig {
        storage,
        ..AppConfig::default()
    };
    let rules = RuleStore::from_compiled(&config.storage, RuleSet::parse("default", "").unwrap());
    SharedState {
        dns_resolver: Arc::new(crate::dns::DnsResolver::new(&config).unwrap()),
        config,
        rules,
        trace: rsproxy_trace::TraceStore::new(8),
        mitm_cert_cache: Arc::new(Mutex::new(MitmCertCache::new(8))),
        mitm_failures: Arc::new(Mutex::new(MitmFailureCache::new(
            8,
            Duration::from_secs(300),
        ))),
        upstream_roots: Arc::new(OnceLock::new()),
        started_ms: rsproxy_trace::now_millis(),
    }
}

pub(super) fn request(method: &str, target: &str, body: &[u8]) -> RawRequest {
    RawRequest {
        method: method.to_string(),
        target: target.to_string(),
        version: "HTTP/1.1".to_string(),
        headers: Vec::new(),
        body: body.to_vec(),
        trailers: Vec::new(),
    }
}

pub(super) fn response_body(response: &[u8]) -> &str {
    let response = std::str::from_utf8(response).unwrap();
    response.split_once("\r\n\r\n").unwrap().1
}
