use super::super::http::RawRequest;
use super::super::{ControlOptions, ControlState};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STORAGE: AtomicU64 = AtomicU64::new(1);

pub(super) fn test_state() -> ControlState {
    let storage = std::env::temp_dir().join(format!(
        "rsproxy-control-routes-{}-{}-{}",
        std::process::id(),
        rsproxy_trace::now_millis(),
        NEXT_STORAGE.fetch_add(1, Ordering::Relaxed)
    ));
    let mut config = rsproxy_engine::ProxyConfig::new(storage.clone());
    config.trace_disk_budget = 0;
    let options = ControlOptions {
        host: "127.0.0.1".to_string(),
        port: 8899,
        api: "127.0.0.1:8900".to_string(),
        api_token: None,
        storage,
        config_path: None,
        rules_watch: config.rules_watch,
        rules_watch_debounce: config.rules_watch_debounce,
        max_header_size: config.max_header_size,
        max_header_count: config.max_header_count,
        max_body_size: config.body_buffer_limit,
    };
    let engine = rsproxy_engine::SharedState::new(config).unwrap();
    ControlState::new(options, engine.handle())
}

pub(super) fn request(method: &str, target: &str, body: &[u8]) -> RawRequest {
    RawRequest {
        method: method.to_string(),
        target: target.to_string(),
        headers: Vec::new(),
        body: body.to_vec(),
    }
}

pub(super) fn response_body(response: &[u8]) -> &str {
    let response = std::str::from_utf8(response).unwrap();
    response.split_once("\r\n\r\n").unwrap().1
}
