use super::*;
use crate::rule_store::RuleStore;
use crate::state::ProxyConfig;
use std::sync::Arc;

fn meta(url: &str) -> RequestMeta {
    RequestMeta {
        method: "GET".to_string(),
        url: url.to_string(),
        headers: Vec::new(),
        body: Vec::new(),
        client_ip: None,
        server_ip: literal_ip_from_url(url),
        template: Default::default(),
    }
}

fn test_state() -> SharedState {
    let config = ProxyConfig::default();
    let rules = RuleStore::from_compiled(
        &config.storage,
        rsproxy_rules::RuleSet::parse("default", "").unwrap(),
    );
    let dns_config = rsproxy_net::DnsConfig {
        servers: config.dns_servers.clone(),
        timeout: config.dns_timeout,
        cache_ttl: config.dns_cache_ttl,
    };
    SharedState::from_test_parts(
        config,
        rules,
        rsproxy_trace::TraceStore::new(8),
        Arc::new(rsproxy_net::DnsResolver::new(&dns_config).unwrap()),
    )
}

fn request_deadline() -> RequestDeadline {
    RequestDeadline::new(Duration::from_secs(5)).unwrap()
}

fn test_connection_input() -> HttpConnectionInput {
    HttpConnectionInput {
        peer: "127.0.0.1:12345".to_string(),
        https_authority: None,
        plain_client_clone: None,
        initial_tls: Vec::new(),
        started_ms_override: None,
        initial_flags: Vec::new(),
        client_connection: ClientPersistence::KeepAlive,
    }
}

fn request_with_proxy_authorization(value: Option<&str>) -> RawRequest {
    RawRequest {
        method: "GET".to_string(),
        target: "http://example.test/".to_string(),
        version: "HTTP/1.1".to_string(),
        headers: value
            .map(|value| vec![("Proxy-Authorization".to_string(), value.to_string())])
            .unwrap_or_default(),
        body: Vec::new(),
        trailers: Vec::new(),
    }
}

fn resolved(action: Action) -> ResolvedAction {
    ResolvedAction::new(
        action,
        rsproxy_rules::MatchedRule {
            group: "default".to_string(),
            line: 1,
            raw: String::new(),
        },
        Default::default(),
    )
}

mod action_effects;
mod connect_modes;
mod connect_proxy;
mod connection;
mod h1_forward;
mod h2_downstream_streaming;
mod h2_tls;
mod header_actions;
mod mock_trace;
mod origin_h2_streaming;
mod protocol_matrix;
mod request_streaming;
mod response_actions;
mod routing;
mod streaming;
mod support;
mod template_actions;
mod timeouts;
mod tls_policy;
mod value_actions;
mod value_runtime_matrix;
mod websocket;
mod websocket_nonblocking;
