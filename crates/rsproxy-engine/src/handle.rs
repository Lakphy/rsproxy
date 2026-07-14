use crate::{EngineResult, RuleStore, SharedState};
use rsproxy_trace::{Session, TraceStore};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone)]
/// Cloneable, thread-safe control boundary for a running engine.
///
/// The handle exposes snapshots and command entry points without revealing the
/// mutable caches and protocol state held by [`SharedState`]. Clones share the
/// rule repository, trace collector, resolver counters, and caches; methods may
/// be called concurrently from independent control connections.
pub struct EngineHandle {
    state: SharedState,
}

impl EngineHandle {
    pub(crate) fn new(state: SharedState) -> Self {
        Self { state }
    }

    /// Borrows the shared rule store used for subsequent requests.
    ///
    /// Readers obtain atomic immutable snapshots; mutations are serialized and
    /// publish only after the prospective complete rule set validates.
    pub fn rules(&self) -> &RuleStore {
        &self.state.rules
    }

    /// Clones the trace-store handle without copying captured sessions or collector state.
    pub fn trace_store(&self) -> TraceStore {
        self.state.trace.clone()
    }

    /// Reads counters and configuration into a point-in-time status value.
    ///
    /// Locks are held only while copying their individual values; the snapshot
    /// is not a transaction across all subsystems.
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
                .expect("MITM failure cache lock poisoned")
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

    /// Reissues captured request data directly to its HTTP or HTTPS origin over HTTP/1.1.
    ///
    /// Replay rejects non-HTTP(S) URLs, bypasses the rule and upstream
    /// routing pipelines, and sends the captured request body prefix, which may
    /// be truncated relative to the original request. Response headers use the
    /// configured resolver, connect, response-head, and request-total timeouts.
    /// The response is read to EOF; [`ReplayResponse::body_head`] retains at
    /// most 64 KiB while [`ReplayResponse::response_bytes`] reports the complete
    /// number of bytes read.
    pub fn replay(&self, session: &Session) -> EngineResult<ReplayResponse> {
        crate::replay::replay_session(session, &self.state)
    }
}

#[derive(Clone, Debug)]
/// Point-in-time engine status assembled for the control plane.
pub struct EngineStatusSnapshot {
    /// Effective non-secret data-plane configuration.
    pub config: EngineConfigStatus,
    /// Unix timestamp in milliseconds captured when the shared state was created.
    pub started_ms: u64,
    /// Number of unexpired host entries in the MITM fallback cache.
    pub mitm_failure_entries: usize,
    /// Upstream trust-root counts, once root initialization has completed.
    pub upstream_roots: Option<UpstreamRootStatus>,
    /// Resolver counters sampled during this status call.
    pub dns: DnsStatusSnapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Non-secret engine configuration projected for status reporting.
pub struct EngineConfigStatus {
    /// Persistent runtime storage root.
    pub storage: PathBuf,
    /// Whether rule-file watching is enabled.
    pub rules_watch: bool,
    /// Filesystem event coalescing interval.
    pub rules_watch_debounce: Duration,
    /// Maximum body bytes aggregated for rule evaluation.
    pub body_buffer_limit: usize,
    /// Whether all CONNECT interception is disabled.
    pub no_mitm: bool,
    /// Whether MITM failures are returned instead of falling back.
    pub strict_mitm: bool,
    /// Maximum in-memory leaf server configurations.
    pub mitm_cert_cache_capacity: usize,
    /// Maximum remembered host-specific MITM failures.
    pub mitm_failure_cache_capacity: usize,
    /// Lifetime of remembered MITM failures.
    pub mitm_failure_ttl: Duration,
    /// Maximum time after CONNECT for classifying initial client bytes.
    pub connect_probe_timeout: Duration,
    /// Maximum active HTTP/1 upstream leases per key.
    pub h1_pool_max_active_per_key: usize,
    /// HTTP/1 keyed-pool admission timeout, measured from lease request.
    pub h1_pool_wait_timeout: Duration,
    /// Maximum active HTTP/2 streams per key.
    pub h2_pool_max_active_streams_per_key: usize,
    /// HTTP/2 stream or connector admission timeout, measured from checkout.
    pub h2_pool_wait_timeout: Duration,
    /// Per-resolution DNS timeout, clipped by request-total time remaining.
    pub dns_timeout: Duration,
    /// Positive and negative DNS cache lifetime.
    pub dns_cache_ttl: Duration,
    /// Explicit resolver endpoints, or empty for system configuration.
    pub dns_servers: Vec<SocketAddr>,
    /// Shared TCP-connect budget across resolved addresses.
    pub tcp_connect_timeout: Duration,
    /// Downstream client TLS handshake timeout.
    pub client_tls_handshake_timeout: Duration,
    /// Upstream origin TLS handshake timeout.
    pub upstream_tls_handshake_timeout: Duration,
    /// Wait after upstream request send for the response head to begin.
    pub upstream_ttfb_timeout: Duration,
    /// Absolute timeout from request dispatch through response completion.
    pub request_total_timeout: Duration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Counts describing the initialized upstream TLS trust store.
pub struct UpstreamRootStatus {
    /// Bundled WebPKI roots accepted into the store.
    pub webpki_roots: usize,
    /// Native roots accepted into the store.
    pub native_loaded: usize,
    /// Native certificates rejected during parsing or insertion.
    pub native_rejected: usize,
    /// Native roots already present in the store.
    pub native_duplicates: usize,
    /// Unique roots available for upstream verification.
    pub total_roots: usize,
    /// Native certificate-loader errors reported by the operating system.
    pub native_errors: usize,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
/// Monotonic DNS counters sampled from the resolver.
pub struct DnsStatusSnapshot {
    /// Resolver calls excluding literal-address bypasses.
    pub lookups: u64,
    /// Lookups that returned at least one address.
    pub successes: u64,
    /// Lookups that completed with a non-timeout error.
    pub failures: u64,
    /// Lookups attributed to the configured DNS deadline.
    pub timeouts: u64,
    /// Targets parsed directly as IP addresses without resolver I/O.
    pub literal_bypasses: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Presentation-neutral result of replaying one captured request.
pub struct ReplayResponse {
    /// Upstream HTTP status code.
    pub status: u16,
    /// Total response-body bytes received, including bytes beyond [`Self::body_head`].
    pub response_bytes: usize,
    /// Upstream response headers in wire order.
    pub headers: Vec<(String, String)>,
    /// Bounded prefix of the upstream response body.
    pub body_head: Vec<u8>,
}
