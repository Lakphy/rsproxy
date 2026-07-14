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
