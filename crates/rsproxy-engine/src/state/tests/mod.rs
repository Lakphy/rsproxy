use super::*;

mod mitm_failures;

fn dummy_server_config() -> Arc<ServerConfig> {
    Arc::new(
        ServerConfig::builder()
            .with_no_client_auth()
            .with_cert_resolver(Arc::new(rustls::server::ResolvesServerCertUsingSni::new())),
    )
}

#[test]
fn mitm_cert_cache_respects_lru_capacity() {
    let mut cache = MitmCertCache::new(2);
    cache.insert("a.test".to_string(), dummy_server_config());
    cache.insert("b.test".to_string(), dummy_server_config());
    assert!(cache.get("a.test").is_some());

    cache.insert("c.test".to_string(), dummy_server_config());

    assert_eq!(cache.len(), 2);
    assert!(cache.get("a.test").is_some());
    assert!(cache.get("b.test").is_none());
    assert!(cache.get("c.test").is_some());
}

#[test]
fn mitm_cert_cache_zero_capacity_disables_storage() {
    let mut cache = MitmCertCache::new(0);
    cache.insert("a.test".to_string(), dummy_server_config());
    assert_eq!(cache.len(), 0);
    assert!(cache.get("a.test").is_none());
}
