use crate::app::AppConfig;
use crate::{CliError, CliResult};
use serde_json::Value;
use std::fmt::Write;

pub(super) fn status(body: &str, json: bool) -> CliResult<String> {
    if json {
        return Ok(body.to_string());
    }
    let value = parse(body, "parse daemon status")?;
    let trace = value.get("trace").unwrap_or(&Value::Null);
    let watch = value.get("rule_watch").unwrap_or(&Value::Null);
    let mitm = value.get("mitm").unwrap_or(&Value::Null);
    let dns = value.get("dns").unwrap_or(&Value::Null);
    let groups = value
        .get("rule_groups")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let mut output = String::new();
    writeln!(
        output,
        "status={} version={} uptime={}",
        string(&value, "status"),
        string(&value, "version"),
        duration(number(&value, "uptime_ms"))
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "proxy=http://{}\napi={} auth={}\nstorage={} config={}",
        string(&value, "proxy"),
        string(&value, "api"),
        value
            .get("api_auth")
            .map(|auth| string(auth, "mode"))
            .unwrap_or("-"),
        string(&value, "storage"),
        nullable_string(&value, "config")
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "rules={} groups={} watch={} reloads={} failures={}",
        number(&value, "rules"),
        groups,
        yes_no(boolean(watch, "enabled")),
        number(watch, "reloads"),
        number(watch, "failures")
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "trace sessions={} pending={} dropped={} memory={} spilled={} disk={}",
        number(trace, "sessions"),
        number(trace, "pending_sessions"),
        number(trace, "dropped") + number(trace, "queue_dropped"),
        bytes(number(trace, "total_memory_bytes")),
        number(trace, "spilled"),
        bytes(number(trace, "spill_bytes"))
    )
    .expect("writing to a string cannot fail");
    write!(
        output,
        "mitm={} failures={} dns={} lookups={} dns_failures={}",
        string(mitm, "mode"),
        number(mitm, "failure_cache_entries"),
        string(dns, "mode"),
        number(dns, "lookups"),
        number(dns, "failures")
    )
    .expect("writing to a string cannot fail");
    Ok(output)
}

/// Renders the effective composition-root configuration after applying the
/// CLI > TOML > built-in precedence. This is resolved locally and does not
/// require a running daemon, so it shows what `run`/`start` would use.
pub(super) fn config(config: &AppConfig, json: bool) -> CliResult<String> {
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

pub(super) fn trace_detail(body: &str, json: bool) -> CliResult<String> {
    if json {
        return Ok(body.to_string());
    }
    let value = parse(body, "parse trace detail")?;
    let mut output = String::new();
    writeln!(
        output,
        "id={} kind={} status={} method={}",
        number(&value, "id"),
        string(&value, "kind"),
        number_or_dash(&value, "status"),
        string(&value, "method")
    )
    .expect("writing to a string cannot fail");
    writeln!(output, "url={}", string(&value, "url")).expect("writing to a string cannot fail");
    writeln!(
        output,
        "client={} upstream={} flags={}",
        string(&value, "client"),
        string(&value, "upstream"),
        string_array(&value, "flags").join(",")
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "timing total={}ms pool_wait={}ms dns={}ms connect={}ms send={}ms ttfb={}ms receive={}ms",
        number(&value, "duration_ms"),
        number(&value, "pool_wait_ms"),
        number(&value, "dns_ms"),
        number(&value, "connect_ms"),
        number_or_dash(&value, "request_send_ms"),
        number(&value, "ttfb_ms"),
        number_or_dash(&value, "response_receive_ms")
    )
    .expect("writing to a string cannot fail");
    writeln!(
        output,
        "bytes request={} response={}",
        number(&value, "request_bytes"),
        number(&value, "response_bytes")
    )
    .expect("writing to a string cannot fail");
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        writeln!(output, "error={error}").expect("writing to a string cannot fail");
    }
    section_rules(&mut output, &value);
    section_headers(&mut output, &value, "Request headers", "req_headers");
    section_body(&mut output, &value, "Request body preview", "req_body_head");
    section_headers(&mut output, &value, "Response headers", "res_headers");
    section_body(
        &mut output,
        &value,
        "Response body preview",
        "res_body_head",
    );
    Ok(output.trim_end().to_string())
}

pub(super) fn rule_mutation(body: &str, action: &str, group: &str) -> CliResult<String> {
    let value = parse(body, "parse rule mutation result")?;
    let rules = value.get("rules").and_then(Value::as_u64);
    Ok(match rules {
        Some(rules) => format!("{action} rule group {group}: {rules} rule(s) active"),
        None => format!("{action} rule group {group}"),
    })
}

pub(super) fn mutation(body: &str, json: bool, message: &str) -> CliResult<String> {
    if json {
        return Ok(body.to_string());
    }
    let value = parse(body, "parse mutation result")?;
    if value.get("ok").and_then(Value::as_bool) == Some(false) {
        return Err(CliError::Usage(format!("operation failed: {body}")));
    }
    let count = value
        .get("cleared")
        .and_then(Value::as_u64)
        .map(|count| format!(": {count} removed"))
        .unwrap_or_default();
    Ok(format!("{message}{count}"))
}

pub(super) fn trace_stats(body: &str, json: bool) -> CliResult<String> {
    if json {
        return Ok(body.to_string());
    }
    let value = parse(body, "parse trace statistics")?;
    Ok(format!(
        "sessions={} pending={} incomplete={} next_id={}\n\
         dropped={} queue_dropped={} memory_dropped={} evicted={} orphan_events={}\n\
         memory={} / {} queue={} / {}\n\
         spilled={} segments={} disk={} / {} compression={} evicted_segments={} spill_errors={}",
        number(&value, "sessions"),
        number(&value, "pending_sessions"),
        number(&value, "incomplete_sessions"),
        number(&value, "next_id"),
        number(&value, "dropped"),
        number(&value, "queue_dropped"),
        number(&value, "queue_memory_dropped"),
        number(&value, "evicted_sessions"),
        number(&value, "orphan_events"),
        bytes(number(&value, "total_memory_bytes")),
        bytes(number(&value, "memory_budget_bytes")),
        bytes(number(&value, "queue_bytes")),
        bytes(number(&value, "queue_memory_budget_bytes")),
        number(&value, "spilled"),
        number(&value, "spill_segments"),
        bytes(number(&value, "spill_bytes")),
        bytes(number(&value, "spill_disk_budget_bytes")),
        string(&value, "spill_compression"),
        number(&value, "spill_evicted_segments"),
        number(&value, "spill_errors")
    ))
}

pub(super) fn replay(body: &str, json: bool) -> CliResult<String> {
    if json {
        return Ok(body.to_string());
    }
    let value = parse(body, "parse replay result")?;
    let mut output = format!(
        "replayed id={} status={} bytes={}\nurl={}",
        number(&value, "id"),
        number_or_dash(&value, "status"),
        number(&value, "response_bytes"),
        string(&value, "url")
    );
    section_headers(&mut output, &value, "Response headers", "headers");
    section_body(&mut output, &value, "Response body preview", "body_head");
    Ok(output.trim_end().to_string())
}

fn section_rules(output: &mut String, value: &Value) {
    let Some(rules) = value.get("rules").and_then(Value::as_array) else {
        return;
    };
    output.push_str("\nMatched rules\n");
    if rules.is_empty() {
        output.push_str("  (none)\n");
    }
    for rule in rules {
        writeln!(
            output,
            "  {}:{} {}",
            string(rule, "group"),
            number(rule, "line"),
            string(rule, "raw")
        )
        .expect("writing to a string cannot fail");
    }
}

fn section_headers(output: &mut String, value: &Value, title: &str, key: &str) {
    let Some(headers) = value.get(key).and_then(Value::as_array) else {
        return;
    };
    writeln!(output, "\n{title}").expect("writing to a string cannot fail");
    if headers.is_empty() {
        output.push_str("  (none)\n");
    }
    for header in headers {
        let Some(pair) = header.as_array() else {
            continue;
        };
        writeln!(
            output,
            "  {}: {}",
            pair.first().and_then(Value::as_str).unwrap_or("-"),
            pair.get(1).and_then(Value::as_str).unwrap_or("")
        )
        .expect("writing to a string cannot fail");
    }
}

fn section_body(output: &mut String, value: &Value, title: &str, key: &str) {
    let Some(body) = value
        .get(key)
        .and_then(Value::as_str)
        .filter(|body| !body.is_empty())
    else {
        return;
    };
    const LIMIT: usize = 2_048;
    let total = body.chars().count();
    let preview = body.chars().take(LIMIT).collect::<String>();
    writeln!(output, "\n{title}\n{preview}").expect("writing to a string cannot fail");
    if total > LIMIT {
        writeln!(
            output,
            "[truncated: showing {LIMIT} of {total} characters; use --json for the full captured preview]"
        )
        .expect("writing to a string cannot fail");
    }
}

fn parse(body: &str, context: &'static str) -> CliResult<Value> {
    serde_json::from_str(body).map_err(|source| CliError::Json { context, source })
}

fn string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("-")
}

fn nullable_string<'a>(value: &'a Value, key: &str) -> &'a str {
    value.get(key).and_then(Value::as_str).unwrap_or("defaults")
}

fn number(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn number_or_dash(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_u64)
        .map_or_else(|| "-".to_string(), |number| number.to_string())
}

fn boolean(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn yes_no(value: bool) -> &'static str {
    if value { "on" } else { "off" }
}

fn string_array<'a>(value: &'a Value, key: &str) -> Vec<&'a str> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect()
}

fn duration(milliseconds: u64) -> String {
    if milliseconds < 1_000 {
        format!("{milliseconds}ms")
    } else {
        format!("{:.1}s", milliseconds as f64 / 1_000.0)
    }
}

fn bytes(value: u64) -> String {
    if value < 1_024 {
        format!("{value}B")
    } else if value < 1_048_576 {
        format!("{:.1}KiB", value as f64 / 1_024.0)
    } else {
        format!("{:.1}MiB", value as f64 / 1_048_576.0)
    }
}

#[cfg(test)]
mod tests;
