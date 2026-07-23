use super::ControlState;
use super::respond_json;
use crate::shapes;
use std::io::Write;

pub(super) fn get<W: Write + ?Sized>(stream: &mut W, state: &ControlState) -> std::io::Result<()> {
    let stats = state.trace.stats();
    let status = state.engine.status_snapshot();
    let config = &status.config;
    let rules = state.engine.rules().snapshot();
    let rule_groups = format!(
        "[{}]",
        rules
            .groups
            .iter()
            .enumerate()
            .map(|(order, group)| format!(
                "{{\"name\":{},\"enabled\":{},\"order\":{}}}",
                shapes::string(&group.name),
                group.enabled,
                order
            ))
            .collect::<Vec<_>>()
            .join(",")
    );
    let upstream_roots = status
        .upstream_roots
        .as_ref()
        .map(|roots| {
            format!(
                "{{\"initialized\":true,\"webpki\":{},\"native_loaded\":{},\"native_rejected\":{},\"native_duplicates\":{},\"total\":{},\"native_errors\":{}}}",
                roots.webpki_roots,
                roots.native_loaded,
                roots.native_rejected,
                roots.native_duplicates,
                roots.total_roots,
                roots.native_errors
            )
        })
        .unwrap_or_else(|| "{\"initialized\":false}".to_string());
    let dns_stats = status.dns;
    let dns_servers = format!(
        "[{}]",
        config
            .dns_servers
            .iter()
            .map(|server| shapes::string(&server.to_string()))
            .collect::<Vec<_>>()
            .join(",")
    );
    let dns = format!(
        "{{\"mode\":{},\"servers\":{},\"cache_ttl_ms\":{},\"lookups\":{},\"successes\":{},\"failures\":{},\"timeouts\":{},\"literal_bypasses\":{}}}",
        shapes::string(if config.dns_servers.is_empty() {
            "system"
        } else {
            "custom"
        }),
        dns_servers,
        config.dns_cache_ttl.as_millis(),
        dns_stats.lookups,
        dns_stats.successes,
        dns_stats.failures,
        dns_stats.timeouts,
        dns_stats.literal_bypasses,
    );
    let config_path = state
        .options
        .config_path
        .as_ref()
        .map(|path| shapes::string(&path.display().to_string()))
        .unwrap_or_else(|| "null".to_string());
    let watch_status = state.engine.rules().watch_status();
    let watch_error = watch_status
        .last_error
        .as_ref()
        .map(|error| shapes::string(error))
        .unwrap_or_else(|| "null".to_string());
    let watch_reload = watch_status
        .last_reload_ms
        .map(|value| value.to_string())
        .unwrap_or_else(|| "null".to_string());
    let rule_watch = format!(
        "{{\"enabled\":{},\"debounce_ms\":{},\"events\":{},\"dropped_events\":{},\"reloads\":{},\"failures\":{},\"last_reload_ms\":{},\"last_error\":{}}}",
        state.options.rules_watch,
        state.options.rules_watch_debounce.as_millis(),
        watch_status.events,
        watch_status.dropped_events,
        watch_status.reloads,
        watch_status.failures,
        watch_reload,
        watch_error
    );
    let mitm_mode = if config.no_mitm {
        "disabled"
    } else if config.strict_mitm {
        "strict"
    } else {
        "auto"
    };
    let mitm = format!(
        "{{\"mode\":{},\"cert_cache_capacity\":{},\"failure_cache_capacity\":{},\"failure_cache_entries\":{},\"failure_ttl_ms\":{},\"connect_probe_timeout_ms\":{}}}",
        shapes::string(mitm_mode),
        config.mitm_cert_cache_capacity,
        config.mitm_failure_cache_capacity,
        status.mitm_failure_entries,
        config.mitm_failure_ttl.as_millis(),
        config.connect_probe_timeout.as_millis(),
    );
    let body = format!(
        "{{\"status\":\"running\",\"version\":{},\"proxy\":\"{}:{}\",\"api\":{},\"api_auth\":{{\"mode\":{}}},\"storage\":{},\"config\":{},\"body_buffer_limit\":{},\"uptime_ms\":{},\"rules\":{},\"rule_groups\":{},\"rule_watch\":{},\"mitm\":{},\"h1_pool\":{{\"max_active_per_key\":{},\"wait_timeout_ms\":{}}},\"h2_pool\":{{\"max_active_streams_per_key\":{},\"wait_timeout_ms\":{}}},\"dns\":{},\"timeouts\":{{\"dns_ms\":{},\"tcp_connect_ms\":{},\"client_tls_handshake_ms\":{},\"upstream_tls_handshake_ms\":{},\"upstream_ttfb_ms\":{},\"request_total_ms\":{}}},\"upstream_roots\":{},\"trace\":{}}}",
        shapes::string(env!("CARGO_PKG_VERSION")),
        state.options.host,
        state.options.port,
        shapes::string(&state.options.api),
        shapes::string(if state.options.api_token.is_some() {
            "token"
        } else {
            "peer"
        }),
        shapes::string(&state.options.storage.display().to_string()),
        config_path,
        config.body_buffer_limit,
        rsproxy_trace::now_millis().saturating_sub(status.started_ms),
        rules.compiled.rules().len(),
        rule_groups,
        rule_watch,
        mitm,
        config.h1_pool_max_active_per_key,
        config.h1_pool_wait_timeout.as_millis(),
        config.h2_pool_max_active_streams_per_key,
        config.h2_pool_wait_timeout.as_millis(),
        dns,
        config.dns_timeout.as_millis(),
        config.tcp_connect_timeout.as_millis(),
        config.client_tls_handshake_timeout.as_millis(),
        config.upstream_tls_handshake_timeout.as_millis(),
        config.upstream_ttfb_timeout.as_millis(),
        config.request_total_timeout.as_millis(),
        upstream_roots,
        shapes::stats(stats)
    );
    respond_json(stream, 200, &body)
}
