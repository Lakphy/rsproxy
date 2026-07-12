use rsproxy_trace::{TraceSpillCompression, TraceStore};
use rustls::{RootCertStore, ServerConfig};
use std::collections::{HashMap, VecDeque};
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

mod mitm_failures;

pub use mitm_failures::MitmFailureCache;

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub config_path: Option<PathBuf>,
    pub host: String,
    pub port: u16,
    pub api: String,
    pub api_token: Option<String>,
    pub storage: PathBuf,
    pub rules_watch: bool,
    pub rules_watch_debounce: Duration,
    pub proxy_auth: Option<String>,
    pub max_header_size: usize,
    pub max_header_count: usize,
    pub body_buffer_limit: usize,
    pub trace_body_limit: usize,
    pub trace_exclude_media_body: bool,
    pub trace_queue_capacity: usize,
    pub trace_memory_budget: usize,
    pub trace_spill_segment_size: usize,
    pub trace_disk_budget: usize,
    pub trace_spill_compression: TraceSpillCompression,
    pub no_mitm: bool,
    pub strict_mitm: bool,
    pub mitm_cert_cache_capacity: usize,
    pub mitm_failure_cache_capacity: usize,
    pub mitm_failure_ttl: Duration,
    pub connect_probe_timeout: Duration,
    pub h1_pool_max_active_per_key: usize,
    pub h1_pool_wait_timeout: Duration,
    pub h2_pool_max_active_streams_per_key: usize,
    pub h2_pool_wait_timeout: Duration,
    pub dns_timeout: Duration,
    pub dns_cache_ttl: Duration,
    pub dns_servers: Vec<SocketAddr>,
    pub tcp_connect_timeout: Duration,
    pub client_tls_handshake_timeout: Duration,
    pub upstream_tls_handshake_timeout: Duration,
    pub upstream_ttfb_timeout: Duration,
    pub request_total_timeout: Duration,
}

#[derive(Clone)]
pub struct SharedState {
    pub config: AppConfig,
    pub rules: crate::rule_store::RuleStore,
    pub trace: TraceStore,
    pub mitm_cert_cache: Arc<Mutex<MitmCertCache>>,
    pub mitm_failures: Arc<Mutex<MitmFailureCache>>,
    pub upstream_roots: Arc<OnceLock<UpstreamRootCache>>,
    pub dns_resolver: Arc<crate::dns::DnsResolver>,
    pub started_ms: u64,
}

#[derive(Clone, Debug)]
pub struct UpstreamRootCache {
    pub roots: RootCertStore,
    pub webpki_roots: usize,
    pub native_loaded: usize,
    pub native_rejected: usize,
    pub native_duplicates: usize,
    pub total_roots: usize,
    pub native_errors: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct MitmCertCache {
    capacity: usize,
    entries: HashMap<String, Arc<ServerConfig>>,
    order: VecDeque<String>,
}

impl MitmCertCache {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub fn get(&mut self, host: &str) -> Option<Arc<ServerConfig>> {
        let entry = self.entries.get(host).cloned()?;
        self.touch(host);
        Some(entry)
    }

    pub fn insert(&mut self, host: String, config: Arc<ServerConfig>) {
        if self.capacity == 0 {
            return;
        }
        if self.entries.contains_key(&host) {
            self.entries.insert(host.clone(), config);
            self.touch(&host);
            return;
        }
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.order.push_back(host.clone());
        self.entries.insert(host, config);
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    fn touch(&mut self, host: &str) {
        self.order.retain(|seen| seen != host);
        self.order.push_back(host.to_string());
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        let storage = default_storage();
        Self {
            config_path: None,
            host: "127.0.0.1".to_string(),
            port: 8899,
            api: default_api_for_storage(&storage),
            api_token: None,
            storage,
            rules_watch: false,
            rules_watch_debounce: Duration::from_millis(200),
            proxy_auth: None,
            max_header_size: 256 * 1024,
            max_header_count: 256,
            body_buffer_limit: 8 * 1024 * 1024,
            trace_body_limit: 64 * 1024,
            trace_exclude_media_body: true,
            trace_queue_capacity: rsproxy_trace::DEFAULT_TRACE_QUEUE_CAPACITY,
            trace_memory_budget: rsproxy_trace::DEFAULT_TRACE_MEMORY_BUDGET,
            trace_spill_segment_size: 64 * 1024 * 1024,
            trace_disk_budget: 2 * 1024 * 1024 * 1024,
            trace_spill_compression: TraceSpillCompression::None,
            no_mitm: false,
            strict_mitm: false,
            mitm_cert_cache_capacity: 1024,
            mitm_failure_cache_capacity: 1024,
            mitm_failure_ttl: Duration::from_secs(300),
            connect_probe_timeout: Duration::from_millis(250),
            h1_pool_max_active_per_key: 256,
            h1_pool_wait_timeout: Duration::from_secs(15),
            h2_pool_max_active_streams_per_key: 256,
            h2_pool_wait_timeout: Duration::from_secs(15),
            dns_timeout: Duration::from_secs(5),
            dns_cache_ttl: Duration::from_secs(60),
            dns_servers: Vec::new(),
            tcp_connect_timeout: Duration::from_secs(10),
            client_tls_handshake_timeout: Duration::from_secs(10),
            upstream_tls_handshake_timeout: Duration::from_secs(10),
            upstream_ttfb_timeout: Duration::from_secs(60),
            request_total_timeout: Duration::from_secs(360),
        }
    }
}

pub fn default_api_for_storage(storage: &std::path::Path) -> String {
    #[cfg(windows)]
    {
        let _ = storage;
        "pipe:rsproxy-control".to_string()
    }
    #[cfg(unix)]
    {
        format!("unix:{}", unix_control_socket_path(storage).display())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = storage;
        "127.0.0.1:8900".to_string()
    }
}

#[cfg(unix)]
fn unix_control_socket_path(storage: &std::path::Path) -> PathBuf {
    use sha2::{Digest, Sha256};

    let local = storage.join("run/ctl.sock");
    if local.to_string_lossy().len() <= 96 {
        return local;
    }
    let digest = Sha256::digest(storage.to_string_lossy().as_bytes());
    let suffix = digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    PathBuf::from("/tmp").join(format!("rsproxy-{}-{suffix}.sock", unsafe {
        libc::geteuid()
    }))
}

pub fn unix_api_path(api: &str) -> Option<&str> {
    api.strip_prefix("unix://")
        .or_else(|| api.strip_prefix("unix:"))
        .filter(|path| !path.is_empty())
}

pub fn windows_pipe_path(api: &str) -> Option<&str> {
    api.strip_prefix("pipe://")
        .or_else(|| api.strip_prefix("pipe:"))
        .or_else(|| api.strip_prefix("npipe://"))
        .or_else(|| api.strip_prefix("npipe:"))
        .filter(|path| !path.is_empty())
}

pub fn api_display(api: &str) -> String {
    if unix_api_path(api).is_some() || windows_pipe_path(api).is_some() {
        api.to_string()
    } else {
        format!("http://{api}")
    }
}

pub fn default_storage() -> PathBuf {
    env::var_os("RSPROXY_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".rsproxy")))
        .unwrap_or_else(|| PathBuf::from(".rsproxy"))
}

#[cfg(test)]
#[path = "app/tests/mod.rs"]
mod tests;
