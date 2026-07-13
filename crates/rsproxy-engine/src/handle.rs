use crate::{EngineResult, RuleStore, SharedState};
use rsproxy_trace::{Session, TraceStore};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone)]
pub struct EngineHandle {
    state: SharedState,
}

impl EngineHandle {
    pub(crate) fn new(state: SharedState) -> Self {
        Self { state }
    }

    pub fn rules(&self) -> &RuleStore {
        &self.state.rules
    }

    pub fn trace_store(&self) -> TraceStore {
        self.state.trace.clone()
    }

    pub fn status_snapshot(&self) -> EngineStatusSnapshot {
        let upstream_roots = self
            .state
            .upstream_roots
            .get()
            .map(|roots| UpstreamRootStatus {
                webpki_roots: roots.webpki_roots,
                native_loaded: roots.native_loaded,
                native_rejected: roots.native_rejected,
                native_duplicates: roots.native_duplicates,
                total_roots: roots.total_roots,
                native_errors: roots.native_errors.len(),
            });
        let dns = self.state.dns_resolver.stats();
        EngineStatusSnapshot {
            config: EngineConfigStatus {
                storage: self.state.config.storage.clone(),
                rules_watch: self.state.config.rules_watch,
                rules_watch_debounce: self.state.config.rules_watch_debounce,
                body_buffer_limit: self.state.config.body_buffer_limit,
                no_mitm: self.state.config.no_mitm,
                strict_mitm: self.state.config.strict_mitm,
                mitm_cert_cache_capacity: self.state.config.mitm_cert_cache_capacity,
                mitm_failure_cache_capacity: self.state.config.mitm_failure_cache_capacity,
                mitm_failure_ttl: self.state.config.mitm_failure_ttl,
                connect_probe_timeout: self.state.config.connect_probe_timeout,
                h1_pool_max_active_per_key: self.state.config.h1_pool_max_active_per_key,
                h1_pool_wait_timeout: self.state.config.h1_pool_wait_timeout,
                h2_pool_max_active_streams_per_key: self
                    .state
                    .config
                    .h2_pool_max_active_streams_per_key,
                h2_pool_wait_timeout: self.state.config.h2_pool_wait_timeout,
                dns_timeout: self.state.config.dns_timeout,
                dns_cache_ttl: self.state.config.dns_cache_ttl,
                dns_servers: self.state.config.dns_servers.clone(),
                tcp_connect_timeout: self.state.config.tcp_connect_timeout,
                client_tls_handshake_timeout: self.state.config.client_tls_handshake_timeout,
                upstream_tls_handshake_timeout: self.state.config.upstream_tls_handshake_timeout,
                upstream_ttfb_timeout: self.state.config.upstream_ttfb_timeout,
                request_total_timeout: self.state.config.request_total_timeout,
            },
            started_ms: self.state.started_ms,
            mitm_failure_entries: self
                .state
                .mitm_failures
                .lock()
                .expect("MITM failure cache poisoned")
                .active_len(),
            upstream_roots,
            dns: DnsStatusSnapshot {
                lookups: dns.lookups,
                successes: dns.successes,
                failures: dns.failures,
                timeouts: dns.timeouts,
                literal_bypasses: dns.literal_bypasses,
            },
        }
    }

    pub fn replay(&self, session: &Session) -> EngineResult<ReplayResponse> {
        crate::replay::replay_session(
            session,
            self.state.config.max_header_size,
            self.state.config.max_header_count,
        )
    }
}

#[derive(Clone, Debug)]
pub struct EngineStatusSnapshot {
    pub config: EngineConfigStatus,
    pub started_ms: u64,
    pub mitm_failure_entries: usize,
    pub upstream_roots: Option<UpstreamRootStatus>,
    pub dns: DnsStatusSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineConfigStatus {
    pub storage: PathBuf,
    pub rules_watch: bool,
    pub rules_watch_debounce: Duration,
    pub body_buffer_limit: usize,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamRootStatus {
    pub webpki_roots: usize,
    pub native_loaded: usize,
    pub native_rejected: usize,
    pub native_duplicates: usize,
    pub total_roots: usize,
    pub native_errors: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DnsStatusSnapshot {
    pub lookups: u64,
    pub successes: u64,
    pub failures: u64,
    pub timeouts: u64,
    pub literal_bypasses: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReplayResponse {
    pub status: u16,
    pub response_bytes: usize,
    pub headers: Vec<(String, String)>,
    pub body_head: Vec<u8>,
}
