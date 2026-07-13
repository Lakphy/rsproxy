use crate::rule_store::RuleWatchHandle;
use crate::{CaMaterial, EngineHandle, EngineResult, RuleStore};
use rsproxy_trace::{TraceSpillCompression, TraceStore};
use rustls::{RootCertStore, ServerConfig};
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

mod mitm_failures;

pub(crate) use mitm_failures::MitmFailureCache;

/// Runtime-only configuration for the proxy data plane.
///
/// Listener addresses, control endpoints and authentication tokens belong to
/// the composition root and are intentionally absent from this type.
#[derive(Clone, Debug)]
pub struct ProxyConfig {
    /// Persistent root for rules, trace spill segments and leaf certificates.
    pub storage: PathBuf,
    /// Root CA material supplied by the composition root. The engine never
    /// discovers this material from `storage`.
    pub ca_material: Option<CaMaterial>,
    /// Enables live rule-file watching after the initial synchronous load.
    pub rules_watch: bool,
    /// Coalescing interval applied to rule filesystem notifications.
    pub rules_watch_debounce: Duration,
    /// Optional `Basic` credential expected from downstream proxy clients.
    pub proxy_auth: Option<String>,
    /// Maximum encoded bytes in an HTTP header or trailer block.
    ///
    /// For HTTP/1 this includes the start line and complete header section; for
    /// HTTP/2 it limits the decoded header list.
    pub max_header_size: usize,
    /// Maximum accepted field count in each HTTP header or trailer block.
    pub max_header_count: usize,
    /// Maximum request or response bytes collected for body-dependent rules and rewrites.
    ///
    /// Larger bodies continue on a streaming path with complete-body operations skipped.
    pub body_buffer_limit: usize,
    /// Maximum body bytes retained per direction in a trace session.
    pub trace_body_limit: usize,
    /// Excludes recognized media payloads from trace bodies when enabled.
    pub trace_exclude_media_body: bool,
    /// Maximum number of pending trace commands.
    pub trace_queue_capacity: usize,
    /// Total in-memory trace budget in bytes, including pending commands.
    pub trace_memory_budget: usize,
    /// Target maximum encoded size of each trace spill segment in bytes.
    pub trace_spill_segment_size: usize,
    /// Total on-disk trace spill budget in bytes; zero disables spilling.
    pub trace_disk_budget: usize,
    /// Compression applied independently to newly written spill records.
    pub trace_spill_compression: TraceSpillCompression,
    /// Passes CONNECT traffic through without attempting MITM inspection.
    pub no_mitm: bool,
    /// Rejects MITM failures instead of remembering and falling back to passthrough.
    pub strict_mitm: bool,
    /// Maximum number of generated leaf server configurations retained in memory.
    pub mitm_cert_cache_capacity: usize,
    /// Maximum number of host-specific MITM failures remembered for fallback.
    pub mitm_failure_cache_capacity: usize,
    /// Lifetime of a remembered MITM failure before inspection is retried.
    pub mitm_failure_ttl: Duration,
    /// Time allowed after a successful CONNECT to classify peeked client bytes as HTTP or TLS.
    pub connect_probe_timeout: Duration,
    /// Maximum simultaneous HTTP/1 upstream leases for one pool key.
    ///
    /// A lease is held through response-body completion because HTTP/1 cannot
    /// multiplex another request on that connection meanwhile.
    pub h1_pool_max_active_per_key: usize,
    /// Maximum wait from requesting HTTP/1 keyed-pool admission to reserving a lease.
    pub h1_pool_wait_timeout: Duration,
    /// Maximum simultaneous HTTP/2 streams for one pool key, held through stream completion.
    pub h2_pool_max_active_streams_per_key: usize,
    /// Maximum wait from HTTP/2 checkout to a stream slot or permission to connect.
    pub h2_pool_wait_timeout: Duration,
    /// Per-resolution DNS timeout, measured from the resolver call and clipped by request total.
    pub dns_timeout: Duration,
    /// Lifetime of positive and negative DNS cache entries.
    pub dns_cache_ttl: Duration,
    /// Explicit recursive DNS servers; an empty list uses system configuration.
    pub dns_servers: Vec<SocketAddr>,
    /// Shared budget for TCP attempts across all addresses returned by one resolution.
    pub tcp_connect_timeout: Duration,
    /// Maximum duration from starting to completing a downstream client TLS handshake.
    pub client_tls_handshake_timeout: Duration,
    /// Maximum duration of each upstream TLS handshake after its TCP route is established.
    pub upstream_tls_handshake_timeout: Duration,
    /// Maximum wait after request send for the upstream response head to begin.
    ///
    /// HTTP/1 records TTFB at the first response byte; HTTP/2 completes this
    /// stage when the response headers become available.
    pub upstream_ttfb_timeout: Duration,
    /// Absolute deadline from data-plane request dispatch through response completion.
    ///
    /// The downstream request head is parsed before this clock starts. Every
    /// later stage budget is clipped to the time remaining.
    pub request_total_timeout: Duration,
}

impl ProxyConfig {
    /// Creates a configuration with production defaults and the supplied storage root.
    pub fn new(storage: impl Into<PathBuf>) -> Self {
        Self {
            storage: storage.into(),
            ..Self::default()
        }
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            storage: PathBuf::from(".rsproxy"),
            ca_material: None,
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

#[derive(Clone)]
/// Thread-safe runtime state shared by connection handlers.
///
/// Clones share atomic rule snapshots, trace collection, the DNS resolver and
/// bounded caches. Configuration is copied but cannot be mutated through the
/// public API. Construction performs the initial rule load and optional watcher
/// setup before any clone can be handed to a connection thread.
pub struct SharedState {
    pub(crate) config: ProxyConfig,
    pub(crate) rules: RuleStore,
    pub(crate) trace: TraceStore,
    pub(crate) mitm_cert_cache: Arc<Mutex<MitmCertCache>>,
    pub(crate) mitm_failures: Arc<Mutex<MitmFailureCache>>,
    pub(crate) upstream_roots: Arc<OnceLock<UpstreamRootCache>>,
    pub(crate) dns_resolver: Arc<rsproxy_net::DnsResolver>,
    pub(crate) started_ms: u64,
    pub(crate) _rule_watch: Arc<Mutex<Option<RuleWatchHandle>>>,
}

impl SharedState {
    /// Builds all runtime-owned resources before publishing a usable state.
    ///
    /// Initial rules and DNS configuration are validated synchronously. When
    /// enabled, the rules watcher is retained for the lifetime of all clones.
    pub fn new(config: ProxyConfig) -> EngineResult<Self> {
        let rules = RuleStore::load(&config.storage)?;
        let rule_watch = if config.rules_watch {
            Some(rules.watch(config.rules_watch_debounce)?)
        } else {
            None
        };
        let dns_config = rsproxy_net::DnsConfig {
            servers: config.dns_servers.clone(),
            timeout: config.dns_timeout,
            cache_ttl: config.dns_cache_ttl,
        };
        let dns_resolver = Arc::new(rsproxy_net::DnsResolver::new(&dns_config)?);
        let trace_spill = (config.trace_disk_budget != 0).then(|| {
            rsproxy_trace::TraceSpillConfig::new(
                config.storage.join("trace"),
                config.trace_spill_segment_size as u64,
                config.trace_disk_budget as u64,
            )
            .with_compression(config.trace_spill_compression)
        });
        let state = Self {
            rules,
            trace: TraceStore::new_with_config(rsproxy_trace::TraceStoreConfig {
                max_sessions: 4096,
                queue_capacity: config.trace_queue_capacity,
                memory_budget_bytes: config.trace_memory_budget,
                queue_memory_budget_bytes: None,
                body_limit: config.trace_body_limit,
                spill: trace_spill,
            }),
            mitm_cert_cache: Arc::new(Mutex::new(MitmCertCache::new(
                config.mitm_cert_cache_capacity,
            ))),
            mitm_failures: Arc::new(Mutex::new(MitmFailureCache::new(
                config.mitm_failure_cache_capacity,
                config.mitm_failure_ttl,
            ))),
            upstream_roots: Arc::new(OnceLock::new()),
            dns_resolver,
            started_ms: rsproxy_trace::now_millis(),
            _rule_watch: Arc::new(Mutex::new(rule_watch)),
            config,
        };
        crate::proxy::initialize_upstream_roots(&state);
        Ok(state)
    }

    /// Creates a cloneable control handle sharing this runtime's mutable subsystems.
    pub fn handle(&self) -> EngineHandle {
        EngineHandle::new(self.clone())
    }

    #[cfg(test)]
    pub(crate) fn from_test_parts(
        config: ProxyConfig,
        rules: RuleStore,
        trace: TraceStore,
        dns_resolver: Arc<rsproxy_net::DnsResolver>,
    ) -> Self {
        Self {
            mitm_cert_cache: Arc::new(Mutex::new(MitmCertCache::new(
                config.mitm_cert_cache_capacity,
            ))),
            mitm_failures: Arc::new(Mutex::new(MitmFailureCache::new(
                config.mitm_failure_cache_capacity,
                config.mitm_failure_ttl,
            ))),
            upstream_roots: Arc::new(OnceLock::new()),
            started_ms: rsproxy_trace::now_millis(),
            _rule_watch: Arc::new(Mutex::new(None)),
            config,
            rules,
            trace,
            dns_resolver,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct UpstreamRootCache {
    pub(crate) roots: RootCertStore,
    pub(crate) webpki_roots: usize,
    pub(crate) native_loaded: usize,
    pub(crate) native_rejected: usize,
    pub(crate) native_duplicates: usize,
    pub(crate) total_roots: usize,
    pub(crate) native_errors: Vec<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct MitmCertCache {
    capacity: usize,
    entries: HashMap<String, Arc<ServerConfig>>,
    order: VecDeque<String>,
}

impl MitmCertCache {
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            capacity,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(crate) fn get(&mut self, host: &str) -> Option<Arc<ServerConfig>> {
        let entry = self.entries.get(host).cloned()?;
        self.touch(host);
        Some(entry)
    }

    pub(crate) fn insert(&mut self, host: String, config: Arc<ServerConfig>) {
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
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    fn touch(&mut self, host: &str) {
        self.order.retain(|seen| seen != host);
        self.order.push_back(host.to_string());
    }
}

#[cfg(test)]
#[path = "state/tests/mod.rs"]
mod tests;
