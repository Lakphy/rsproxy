use super::RuntimeArgs;
use std::path::Path;
use std::process::Command;

pub(super) fn append_runtime_arguments(command: &mut Command, args: &RuntimeArgs) {
    append_display(command, "--port", args.port);
    append_string(command, "--host", args.host.as_deref());
    append_string(command, "--api", args.client.api.as_deref());
    append_string(command, "--api-token", args.client.api_token.as_deref());
    append_path(command, "--storage", args.client.storage.as_deref());
    append_path(command, "--config", args.client.config.as_deref());
    append_flag(command, "--watch", args.watch);
    append_display(command, "--watch-debounce-ms", args.watch_debounce_ms);
    append_string(command, "--proxy-auth", args.proxy_auth.as_deref());
    append_string(
        command,
        "--max-header-size",
        args.max_header_size.as_deref(),
    );
    append_display(command, "--max-header-count", args.max_header_count);
    append_string(
        command,
        "--body-buffer-limit",
        args.body_buffer_limit.as_deref(),
    );
    append_string(
        command,
        "--trace-body-limit",
        args.trace_body_limit.as_deref(),
    );
    append_string(command, "--trace-filter", args.trace_filter.as_deref());
    append_display(command, "--trace-queue-capacity", args.trace_queue_capacity);
    append_string(
        command,
        "--trace-mem-budget",
        args.trace_mem_budget.as_deref(),
    );
    append_string(
        command,
        "--trace-segment-size",
        args.trace_segment_size.as_deref(),
    );
    append_string(
        command,
        "--trace-disk-budget",
        args.trace_disk_budget.as_deref(),
    );
    append_string(
        command,
        "--trace-spill-compression",
        args.trace_spill_compression.as_deref(),
    );
    append_flag(command, "--no-mitm", args.no_mitm);
    append_flag(command, "--strict-mitm", args.strict_mitm);
    append_display(
        command,
        "--mitm-cert-cache-capacity",
        args.mitm_cert_cache_capacity,
    );
    append_display(
        command,
        "--mitm-failure-cache-capacity",
        args.mitm_failure_cache_capacity,
    );
    append_display(
        command,
        "--mitm-failure-ttl-seconds",
        args.mitm_failure_ttl_seconds,
    );
    append_display(
        command,
        "--connect-probe-timeout-ms",
        args.connect_probe_timeout_ms,
    );
    append_display(
        command,
        "--h1-pool-max-active-per-key",
        args.h1_pool_max_active_per_key,
    );
    append_display(
        command,
        "--h1-pool-wait-timeout-ms",
        args.h1_pool_wait_timeout_ms,
    );
    append_display(
        command,
        "--h2-pool-max-active-streams-per-key",
        args.h2_pool_max_active_streams_per_key,
    );
    append_display(
        command,
        "--h2-pool-wait-timeout-ms",
        args.h2_pool_wait_timeout_ms,
    );
    append_display(
        command,
        "--tcp-connect-timeout-ms",
        args.tcp_connect_timeout_ms,
    );
    append_display(command, "--dns-timeout-ms", args.dns_timeout_ms);
    append_display(command, "--dns-cache", args.dns_cache);
    for server in &args.dns_server {
        command.args(["--dns-server", server]);
    }
    append_display(
        command,
        "--client-tls-handshake-timeout-ms",
        args.client_tls_handshake_timeout_ms,
    );
    append_display(
        command,
        "--upstream-tls-handshake-timeout-ms",
        args.upstream_tls_handshake_timeout_ms,
    );
    append_display(
        command,
        "--upstream-ttfb-timeout-ms",
        args.upstream_ttfb_timeout_ms,
    );
    append_display(command, "--request-timeout-ms", args.request_timeout_ms);
    append_flag(command, "--no-trace-body", args.no_trace_body);
}

fn append_string(command: &mut Command, name: &str, value: Option<&str>) {
    if let Some(value) = value {
        command.args([name, value]);
    }
}

fn append_path(command: &mut Command, name: &str, value: Option<&Path>) {
    if let Some(value) = value {
        command.arg(name).arg(value);
    }
}

fn append_display<T: ToString>(command: &mut Command, name: &str, value: Option<T>) {
    if let Some(value) = value {
        command.arg(name).arg(value.to_string());
    }
}

fn append_flag(command: &mut Command, name: &str, enabled: bool) {
    if enabled {
        command.arg(name);
    }
}
