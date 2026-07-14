use super::{bytes, duration, yes_no};
use crate::app::AppConfig;
use crate::{CliError, CliResult};
use std::fmt::Write;

/// Renders the effective composition-root configuration after applying the
/// CLI > TOML > built-in precedence. This is resolved locally and does not
/// require a running daemon, so it shows what `run`/`start` would use.
pub(in crate::cli) fn config(config: &AppConfig, json: bool) -> CliResult<String> {
    let engine = config.engine();
    let compression = match engine.trace_spill_compression {
        rsproxy_trace::TraceSpillCompression::None => "none".to_string(),
        rsproxy_trace::TraceSpillCompression::Zstd { level } => format!("zstd:{level}"),
    };
    let dns_servers: Vec<String> = engine
        .dns_servers
        .iter()
        .map(|server| server.to_string())
        .collect();
    if json {
        let value = serde_json::json!({
            "config_path": config.config_path.as_ref().map(|path| path.display().to_string()),
            "listener": { "host": config.host, "port": config.port },
            "control": {
                "api": config.api,
                "api_token": config.api_token.is_some(),
                "storage": engine.storage.display().to_string(),
            },
            "rules": {
                "watch": engine.rules_watch,
                "watch_debounce_ms": engine.rules_watch_debounce.as_millis(),
                "proxy_auth": engine.proxy_auth.is_some(),
            },
            "limits": {
                "max_header_size": engine.max_header_size,
                "max_header_count": engine.max_header_count,
                "body_buffer_limit": engine.body_buffer_limit,
            },
            "trace": {
                "body_limit": engine.trace_body_limit,
                "exclude_media_body": engine.trace_exclude_media_body,
                "queue_capacity": engine.trace_queue_capacity,
                "memory_budget": engine.trace_memory_budget,
                "spill_segment_size": engine.trace_spill_segment_size,
                "disk_budget": engine.trace_disk_budget,
                "spill_compression": compression,
            },
            "mitm": {
                "disabled": engine.no_mitm,
                "strict": engine.strict_mitm,
                "cert_cache_capacity": engine.mitm_cert_cache_capacity,
                "failure_cache_capacity": engine.mitm_failure_cache_capacity,
                "failure_ttl_ms": engine.mitm_failure_ttl.as_millis(),
                "connect_probe_timeout_ms": engine.connect_probe_timeout.as_millis(),
            },
            "pools": {
                "h1_max_active_per_key": engine.h1_pool_max_active_per_key,
                "h1_wait_timeout_ms": engine.h1_pool_wait_timeout.as_millis(),
                "h2_max_active_streams_per_key": engine.h2_pool_max_active_streams_per_key,
                "h2_wait_timeout_ms": engine.h2_pool_wait_timeout.as_millis(),
            },
            "timeouts": {
                "tcp_connect_ms": engine.tcp_connect_timeout.as_millis(),
                "client_tls_handshake_ms": engine.client_tls_handshake_timeout.as_millis(),
                "upstream_tls_handshake_ms": engine.upstream_tls_handshake_timeout.as_millis(),
                "upstream_ttfb_ms": engine.upstream_ttfb_timeout.as_millis(),
                "request_total_ms": engine.request_total_timeout.as_millis(),
            },
            "dns": {
                "timeout_ms": engine.dns_timeout.as_millis(),
                "cache_ttl_ms": engine.dns_cache_ttl.as_millis(),
                "servers": dns_servers,
            },
        });
        return serde_json::to_string_pretty(&value).map_err(|source| CliError::Json {
            context: "serialize effective config",
            source,
        });
    }
    let servers = if dns_servers.is_empty() {
        "system".to_string()
    } else {
        dns_servers.join(",")
    };
    let millis = |value: std::time::Duration| duration(value.as_millis() as u64);
    let mut output = String::new();
    writeln!(
        output,
        "config={}",
        config
            .config_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "built-in defaults (no file loaded)".to_string())
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "listener host={} port={}\ncontrol api={} api_token={} storage={}",
        config.host,
        config.port,
        config.api,
        yes_no(config.api_token.is_some()),
        engine.storage.display(),
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "rules watch={} debounce={} proxy_auth={}",
        yes_no(engine.rules_watch),
        millis(engine.rules_watch_debounce),
        yes_no(engine.proxy_auth.is_some()),
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "limits header_size={} header_count={} body_buffer={}",
        bytes(engine.max_header_size as u64),
        engine.max_header_count,
        bytes(engine.body_buffer_limit as u64),
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "trace body_limit={} exclude_media={} queue={} mem={} segment={} disk={} compression={}",
        bytes(engine.trace_body_limit as u64),
        yes_no(engine.trace_exclude_media_body),
        engine.trace_queue_capacity,
        bytes(engine.trace_memory_budget as u64),
        bytes(engine.trace_spill_segment_size as u64),
        bytes(engine.trace_disk_budget as u64),
        compression,
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "mitm disabled={} strict={} cert_cache={} failure_cache={} failure_ttl={} connect_probe={}",
        yes_no(engine.no_mitm),
        yes_no(engine.strict_mitm),
        engine.mitm_cert_cache_capacity,
        engine.mitm_failure_cache_capacity,
        millis(engine.mitm_failure_ttl),
        millis(engine.connect_probe_timeout),
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "pools h1_active={} h1_wait={} h2_streams={} h2_wait={}",
        engine.h1_pool_max_active_per_key,
        millis(engine.h1_pool_wait_timeout),
        engine.h2_pool_max_active_streams_per_key,
        millis(engine.h2_pool_wait_timeout),
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "timeouts tcp={} client_tls={} upstream_tls={} ttfb={} request_total={}",
        millis(engine.tcp_connect_timeout),
        millis(engine.client_tls_handshake_timeout),
        millis(engine.upstream_tls_handshake_timeout),
        millis(engine.upstream_ttfb_timeout),
        millis(engine.request_total_timeout),
    )
    .expect("writing to a string cannot fail");
    write!(
        output,
        "dns timeout={} cache_ttl={} servers={}",
        millis(engine.dns_timeout),
        millis(engine.dns_cache_ttl),
        servers,
    )
    .expect("writing to a string cannot fail");
    Ok(output)
}
