use super::*;
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(super) struct FileConfig {
    host: Option<String>,
    port: Option<u16>,
    api: Option<String>,
    api_token: Option<String>,
    storage: Option<PathBuf>,
    watch: Option<bool>,
    watch_debounce_ms: Option<u64>,
    proxy_auth: Option<String>,
    max_header_size: Option<SizeValue>,
    max_header_count: Option<usize>,
    body_buffer_limit: Option<SizeValue>,
    trace_body_limit: Option<SizeValue>,
    trace_filter: Option<String>,
    trace_queue_capacity: Option<usize>,
    trace_mem_budget: Option<SizeValue>,
    trace_segment_size: Option<SizeValue>,
    trace_disk_budget: Option<SizeValue>,
    trace_spill_compression: Option<String>,
    no_trace_body: Option<bool>,
    no_mitm: Option<bool>,
    strict_mitm: Option<bool>,
    mitm_cert_cache_capacity: Option<usize>,
    mitm_failure_cache_capacity: Option<usize>,
    mitm_failure_ttl_seconds: Option<u64>,
    connect_probe_timeout_ms: Option<u64>,
    h1_pool_max_active_per_key: Option<usize>,
    h1_pool_wait_timeout_ms: Option<u64>,
    h2_pool_max_active_streams_per_key: Option<usize>,
    h2_pool_wait_timeout_ms: Option<u64>,
    dns_timeout_ms: Option<u64>,
    #[serde(alias = "dns_cache")]
    dns_cache_seconds: Option<u64>,
    #[serde(alias = "dns_servers")]
    dns_server: Option<Vec<String>>,
    tcp_connect_timeout_ms: Option<u64>,
    client_tls_handshake_timeout_ms: Option<u64>,
    upstream_tls_handshake_timeout_ms: Option<u64>,
    upstream_ttfb_timeout_ms: Option<u64>,
    #[serde(alias = "request_total_timeout_ms")]
    request_timeout_ms: Option<u64>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SizeValue {
    Bytes(usize),
    Text(String),
}

impl SizeValue {
    fn parse(self, _field: &str) -> Result<usize, ConfigError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::Text(value) => parse_size(&value),
        }
    }
}

impl FileConfig {
    pub(super) fn has_explicit_api(&self) -> bool {
        self.api.is_some()
    }

    pub(super) fn apply(self, config: &mut AppConfig) -> CliResult<()> {
        if let Some(host) = self.host {
            config.host = host;
        }
        if let Some(port) = self.port {
            config.port = port;
        }
        if let Some(api) = self.api {
            config.api = api;
        }
        if let Some(storage) = self.storage {
            config.engine_mut().storage = storage;
        }
        if let Some(watch) = self.watch {
            config.engine_mut().rules_watch = watch;
        }
        if let Some(value) = self.watch_debounce_ms {
            config.engine_mut().rules_watch_debounce = positive_millis(value, "watch_debounce_ms")?;
        }
        if let Some(token) = self.api_token {
            config.api_token = Some(validate_api_token(&token)?);
        }
        if let Some(auth) = self.proxy_auth {
            config.engine_mut().proxy_auth = Some(parse_proxy_auth(&auth)?);
        }
        if let Some(value) = self.max_header_size {
            config.engine_mut().max_header_size = value.parse("max_header_size")?;
        }
        if let Some(value) = self.max_header_count {
            config.engine_mut().max_header_count = positive_usize(value, "max_header_count")?;
        }
        if let Some(value) = self.body_buffer_limit {
            config.engine_mut().body_buffer_limit =
                positive_size(value.parse("body_buffer_limit")?, "body_buffer_limit")?;
        }
        if let Some(value) = self.trace_body_limit {
            config.engine_mut().trace_body_limit = value.parse("trace_body_limit")?;
        }
        if let Some(filter) = self.trace_filter {
            apply_trace_filter(config, &filter)?;
        }
        if let Some(capacity) = self.trace_queue_capacity {
            config.engine_mut().trace_queue_capacity =
                positive_usize(capacity, "trace_queue_capacity")?;
        }
        if let Some(value) = self.trace_mem_budget {
            config.engine_mut().trace_memory_budget =
                positive_size(value.parse("trace_mem_budget")?, "trace_mem_budget")?;
        }
        if let Some(value) = self.trace_segment_size {
            config.engine_mut().trace_spill_segment_size =
                positive_size(value.parse("trace_segment_size")?, "trace_segment_size")?;
        }
        if let Some(value) = self.trace_disk_budget {
            config.engine_mut().trace_disk_budget = value.parse("trace_disk_budget")?;
        }
        if let Some(compression) = self.trace_spill_compression {
            config.engine_mut().trace_spill_compression =
                parse_trace_spill_compression(&compression)?;
        }
        if let Some(no_mitm) = self.no_mitm {
            config.engine_mut().no_mitm = no_mitm;
        }
        if let Some(strict_mitm) = self.strict_mitm {
            config.engine_mut().strict_mitm = strict_mitm;
        }
        if let Some(capacity) = self.mitm_cert_cache_capacity {
            config.engine_mut().mitm_cert_cache_capacity = capacity;
        }
        if let Some(capacity) = self.mitm_failure_cache_capacity {
            config.engine_mut().mitm_failure_cache_capacity = capacity;
        }
        if let Some(value) = self.mitm_failure_ttl_seconds {
            config.engine_mut().mitm_failure_ttl =
                positive_seconds(value, "mitm_failure_ttl_seconds")?;
        }
        if let Some(value) = self.connect_probe_timeout_ms {
            config.engine_mut().connect_probe_timeout =
                positive_millis(value, "connect_probe_timeout_ms")?;
        }
        if let Some(value) = self.h1_pool_max_active_per_key {
            config.engine_mut().h1_pool_max_active_per_key =
                positive_usize(value, "h1_pool_max_active_per_key")?;
        }
        if let Some(value) = self.h1_pool_wait_timeout_ms {
            config.engine_mut().h1_pool_wait_timeout =
                positive_millis(value, "h1_pool_wait_timeout_ms")?;
        }
        if let Some(value) = self.h2_pool_max_active_streams_per_key {
            config.engine_mut().h2_pool_max_active_streams_per_key =
                positive_usize(value, "h2_pool_max_active_streams_per_key")?;
        }
        if let Some(value) = self.h2_pool_wait_timeout_ms {
            config.engine_mut().h2_pool_wait_timeout =
                positive_millis(value, "h2_pool_wait_timeout_ms")?;
        }
        if let Some(value) = self.dns_timeout_ms {
            config.engine_mut().dns_timeout = positive_millis(value, "dns_timeout_ms")?;
        }
        if let Some(value) = self.dns_cache_seconds {
            config.engine_mut().dns_cache_ttl = Duration::from_secs(value);
        }
        if let Some(servers) = self.dns_server {
            config.engine_mut().dns_servers = dns::parse_dns_servers(&servers)?;
        }
        if let Some(value) = self.tcp_connect_timeout_ms {
            config.engine_mut().tcp_connect_timeout =
                positive_millis(value, "tcp_connect_timeout_ms")?;
        }
        if let Some(value) = self.client_tls_handshake_timeout_ms {
            config.engine_mut().client_tls_handshake_timeout =
                positive_millis(value, "client_tls_handshake_timeout_ms")?;
        }
        if let Some(value) = self.upstream_tls_handshake_timeout_ms {
            config.engine_mut().upstream_tls_handshake_timeout =
                positive_millis(value, "upstream_tls_handshake_timeout_ms")?;
        }
        if let Some(value) = self.upstream_ttfb_timeout_ms {
            config.engine_mut().upstream_ttfb_timeout =
                positive_millis(value, "upstream_ttfb_timeout_ms")?;
        }
        if let Some(value) = self.request_timeout_ms {
            config.engine_mut().request_total_timeout =
                positive_millis(value, "request_timeout_ms")?;
        }
        if self.no_trace_body == Some(true) {
            config.engine_mut().trace_body_limit = 0;
        }
        Ok(())
    }
}
