use super::respond_json;
use crate::app::SharedState;
use crate::json;
use std::io::Write;

pub(super) fn get<W: Write + ?Sized>(stream: &mut W, state: &SharedState) -> std::io::Result<()> {
    let stats = state.trace.stats();
    let rules = state.rules.snapshot();
    let rule_groups = format!(
        "[{}]",
        rules
            .groups
            .iter()
            .enumerate()
            .map(|(order, group)| format!(
                "{{\"name\":{},\"enabled\":{},\"order\":{}}}",
                json::string(&group.name),
                group.enabled,
                order
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let upstream_roots = state
        .upstream_roots
        .get()
        .map(|roots| {
            format!(
                "{{\"initialized\":true,\"webpki\":{},\"native_loaded\":{},\"native_rejected\":{},\"native_duplicates\":{},\"total\":{},\"native_errors\":{}}}",
                roots.webpki_roots,
                roots.native_loaded,
                roots.native_rejected,
                roots.native_duplicates,
                roots.total_roots,
                roots.native_errors.len()
            )
        })
        .unwrap_or_else(|| "{\"initialized\":false}".to_string());
    let dns_stats = state.dns_resolver.stats();
    let dns_servers = format!(
        "[{}]",
        state
            .config
            .dns_servers
            .iter()
            .map(|server| json::string(&server.to_string()))
            .collect::<Vec<_>>()
            .join(",")
    );
    let dns = format!(
        "{{\"mode\":{},\"servers\":{},\"cache_ttl_ms\":{},\"lookups\":{},\"successes\":{},\"failures\":{},\"timeouts\":{},\"literal_bypasses\":{}}}",
        json::string(if state.config.dns_servers.is_empty() {
            "system"
        } else {
            "custom"
        }),
        dns_servers,
        state.config.dns_cache_ttl.as_millis(),
        dns_stats.lookups,
        dns_stats.successes,
        dns_stats.failures,
        dns_stats.timeouts,
        dns_stats.literal_bypasses,
    );
    let config_path = state
        .config
        .config_path
        .as_ref()
        .map(|path| json::string(&path.display().to_string()))
        .unwrap_or_else(|| "null".to_string());
    let watch_status = state.rules.watch_status();
    let watch_error = watch_status
        .last_error
        .as_ref()
        .map(|error| json::string(error))
        .unwrap_or_else(|| "null".to_string());
    let watch_reload = watch_status
        .last_reload_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let rule_watch = format!(
        "{{\"enabled\":{},\"debounce_ms\":{},\"events\":{},\"dropped_events\":{},\"reloads\":{},\"failures\":{},\"last_reload_ms\":{},\"last_error\":{}}}",
        state.config.rules_watch,
        state.config.rules_watch_debounce.as_millis(),
        watch_status.events,
        watch_status.dropped_events,
        watch_status.reloads,
        watch_status.failures,
        watch_reload,
        watch_error
    );
    let mitm_mode = if state.config.no_mitm {
        "disabled"
    } else if state.config.strict_mitm {
        "strict"
    } else {
        "auto"
    };
    let mitm_failure_entries = state.mitm_failures.lock().unwrap().active_len();
    let mitm = format!(
        "{{\"mode\":{},\"cert_cache_capacity\":{},\"failure_cache_capacity\":{},\"failure_cache_entries\":{},\"failure_ttl_ms\":{},\"connect_probe_timeout_ms\":{}}}",
        json::string(mitm_mode),
        state.config.mitm_cert_cache_capacity,
        state.config.mitm_failure_cache_capacity,
        mitm_failure_entries,
        state.config.mitm_failure_ttl.as_millis(),
        state.config.connect_probe_timeout.as_millis(),
    );
    let body = format!(
        "{{\"status\":\"running\",\"version\":{},\"proxy\":\"{}:{}\",\"api\":{},\"api_auth\":{{\"mode\":{}}},\"storage\":{},\"config\":{},\"body_buffer_limit\":{},\"uptime_ms\":{},\"rules\":{},\"rule_groups\":{},\"rule_watch\":{},\"mitm\":{},\"h1_pool\":{{\"max_active_per_key\":{},\"wait_timeout_ms\":{}}},\"h2_pool\":{{\"max_active_streams_per_key\":{},\"wait_timeout_ms\":{}}},\"dns\":{},\"timeouts\":{{\"dns_ms\":{},\"tcp_connect_ms\":{},\"client_tls_handshake_ms\":{},\"upstream_tls_handshake_ms\":{},\"upstream_ttfb_ms\":{},\"request_total_ms\":{}}},\"upstream_roots\":{},\"trace\":{}}}",
        json::string(env!("CARGO_PKG_VERSION")),
        state.config.host,
        state.config.port,
        json::string(&state.config.api),
        json::string(if state.config.api_token.is_some() {
            "token"
        } else {
            "peer"
        }),
        json::string(&state.config.storage.display().to_string()),
        config_path,
        state.config.body_buffer_limit,
        rsproxy_trace::now_millis().saturating_sub(state.started_ms),
        rules.compiled.rules.len(),
        rule_groups,
        rule_watch,
        mitm,
        state.config.h1_pool_max_active_per_key,
        state.config.h1_pool_wait_timeout.as_millis(),
        state.config.h2_pool_max_active_streams_per_key,
        state.config.h2_pool_wait_timeout.as_millis(),
        dns,
        state.config.dns_timeout.as_millis(),
        state.config.tcp_connect_timeout.as_millis(),
        state.config.client_tls_handshake_timeout.as_millis(),
        state.config.upstream_tls_handshake_timeout.as_millis(),
        state.config.upstream_ttfb_timeout.as_millis(),
        state.config.request_total_timeout.as_millis(),
        upstream_roots,
        json::stats(stats)
    );
    respond_json(stream, 200, &body)
}
