use super::state::{DetailTab, TuiApp, TuiSnapshot};
use ratatui::style::{Color, Style};
use ratatui::widgets::Paragraph;
use serde_json::Value as JsonValue;

pub(super) fn footer(app: &TuiApp) -> Paragraph<'static> {
    let filter = if app.filter.is_empty() {
        "<none>".to_string()
    } else {
        app.filter.clone()
    };
    let editing = if app.editing_filter { " editing" } else { "" };
    let replay = app
        .replay_status
        .as_deref()
        .map(|status| format!(" | {status}"))
        .unwrap_or_default();
    Paragraph::new(format!(
        "q quit | R refresh | r replay | / filter{editing}: {filter} | tab {} | up/down select{replay}",
        app.detail_tab.name()
    ))
    .style(Style::default().fg(Color::DarkGray))
}

pub(super) fn plain_snapshot(
    snapshot: &TuiSnapshot,
    detail_tab: DetailTab,
    filter: &str,
    replay_status: Option<&str>,
) -> String {
    let trace = snapshot.status.get("trace").unwrap_or(&JsonValue::Null);
    let mut output = String::new();
    output.push_str("RSPROXY TUI SNAPSHOT\n");
    output.push_str(&format!(
        "status={} proxy={} api={} storage={}\n",
        json_str(&snapshot.status, "status").unwrap_or("-"),
        json_str(&snapshot.status, "proxy").unwrap_or("-"),
        json_str(&snapshot.status, "api").unwrap_or("-"),
        json_str(&snapshot.status, "storage").unwrap_or("-")
    ));
    output.push_str(&format!(
        "trace sessions={} spilled={} dropped={} spill_compression={} spill_errors={}\n",
        json_u64(trace, "sessions").unwrap_or(0),
        json_u64(trace, "spilled").unwrap_or(0),
        json_u64(trace, "dropped").unwrap_or(0),
        json_str(trace, "spill_compression").unwrap_or("none"),
        json_u64(trace, "spill_errors").unwrap_or(0)
    ));
    output.push_str(&format!(
        "filter={} tab={}\n",
        if filter.is_empty() { "<none>" } else { filter },
        detail_tab.name()
    ));
    if let Some(status) = replay_status {
        output.push_str(&format!("replay={status}\n"));
    }
    if let Some(error) = &snapshot.error {
        output.push_str(&format!("error={error}\n"));
    }
    output.push_str("ID    KIND       STATUS DUR_MS  BYTES   METHOD  URL\n");
    for session in &snapshot.sessions {
        output.push_str(&format!(
            "{:<5} {:<10} {:<6} {:<7} {:<7} {:<7} {}\n",
            json_u64(session, "id").unwrap_or(0),
            json_str(session, "kind").unwrap_or("-"),
            json_u64(session, "status").map_or("-".to_string(), |value| value.to_string()),
            json_u64(session, "duration_ms").unwrap_or(0),
            json_u64(session, "response_bytes").unwrap_or(0),
            json_str(session, "method").unwrap_or("-"),
            truncate(json_str(session, "url").unwrap_or("-"), 100)
        ));
    }
    if let Some(detail) = &snapshot.selected_detail {
        output.push_str(&format!(
            "selected id={} upstream={} flags={} error={} tab={}\n",
            json_u64(detail, "id").unwrap_or(0),
            json_str(detail, "upstream").unwrap_or("-"),
            json_array_strings(detail, "flags").join(","),
            json_str(detail, "error").unwrap_or(""),
            detail_tab.name()
        ));
        output.push_str(&plain_detail(detail, detail_tab));
    }
    output
}

pub(super) fn plain_detail(detail: &JsonValue, tab: DetailTab) -> String {
    match tab {
        DetailTab::Overview => overview_detail(detail),
        DetailTab::Headers => headers_detail(detail),
        DetailTab::Body => body_detail(detail),
        DetailTab::Rules => rules_detail(detail),
    }
}

fn overview_detail(detail: &JsonValue) -> String {
    let flags = json_array_strings(detail, "flags").join(",");
    let error = json_str(detail, "error").unwrap_or("");
    format!(
        "id: {}\nkind: {}\nstatus: {}\nmethod: {}\nurl: {}\nupstream: {}\nclient: {}\ntiming: total={}ms pool_wait={}ms dns={}ms connect={}ms send={}ms ttfb={}ms receive={}ms\nbytes: req={} res={}\nflags: {}\nerror: {}\n",
        json_u64(detail, "id").unwrap_or(0),
        json_str(detail, "kind").unwrap_or("-"),
        json_u64(detail, "status").map_or("-".to_string(), |value| value.to_string()),
        json_str(detail, "method").unwrap_or("-"),
        json_str(detail, "url").unwrap_or("-"),
        json_str(detail, "upstream").unwrap_or("-"),
        json_str(detail, "client").unwrap_or("-"),
        json_u64(detail, "duration_ms").unwrap_or(0),
        json_u64(detail, "pool_wait_ms").unwrap_or(0),
        json_u64(detail, "dns_ms").unwrap_or(0),
        json_u64(detail, "connect_ms").unwrap_or(0),
        optional_millis(detail, "request_send_ms"),
        json_u64(detail, "ttfb_ms").unwrap_or(0),
        optional_millis(detail, "response_receive_ms"),
        json_u64(detail, "request_bytes").unwrap_or(0),
        json_u64(detail, "response_bytes").unwrap_or(0),
        flags,
        error,
    )
}

fn optional_millis(detail: &JsonValue, name: &str) -> String {
    json_u64(detail, name)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn headers_detail(detail: &JsonValue) -> String {
    let mut output = String::new();
    output.push_str("Request headers\n");
    for (name, value) in json_header_pairs(detail, "req_headers") {
        output.push_str(&format!("{name}: {value}\n"));
    }
    output.push_str("\nRequest trailers\n");
    for (name, value) in json_header_pairs(detail, "req_trailers") {
        output.push_str(&format!("{name}: {value}\n"));
    }
    output.push_str("\nResponse headers\n");
    for (name, value) in json_header_pairs(detail, "res_headers") {
        output.push_str(&format!("{name}: {value}\n"));
    }
    output.push_str("\nResponse trailers\n");
    for (name, value) in json_header_pairs(detail, "res_trailers") {
        output.push_str(&format!("{name}: {value}\n"));
    }
    if output.trim().is_empty() {
        output.push_str("no headers\n");
    }
    output
}

fn body_detail(detail: &JsonValue) -> String {
    format!(
        "Request body preview\n{}\n\nResponse body preview\n{}\n",
        json_str(detail, "req_body_head").unwrap_or(""),
        json_str(detail, "res_body_head").unwrap_or("")
    )
}

fn rules_detail(detail: &JsonValue) -> String {
    let mut output = String::new();
    for rule in detail
        .get("rules")
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
    {
        output.push_str(&format!(
            "{}:{} {}\n",
            json_str(rule, "group").unwrap_or("-"),
            json_u64(rule, "line").unwrap_or(0),
            json_str(rule, "raw").unwrap_or("")
        ));
    }
    if output.is_empty() {
        output.push_str("no matched rules\n");
    }
    output
}

pub(super) fn json_str<'a>(value: &'a JsonValue, key: &str) -> Option<&'a str> {
    value.get(key).and_then(JsonValue::as_str)
}

pub(super) fn json_u64(value: &JsonValue, key: &str) -> Option<u64> {
    value.get(key).and_then(JsonValue::as_u64)
}

pub(super) fn json_array_strings(value: &JsonValue, key: &str) -> Vec<String> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(JsonValue::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn json_header_pairs(value: &JsonValue, key: &str) -> Vec<(String, String)> {
    value
        .get(key)
        .and_then(JsonValue::as_array)
        .map(|headers| {
            headers
                .iter()
                .filter_map(|pair| {
                    let pair = pair.as_array()?;
                    let name = pair.first()?.as_str()?.to_string();
                    let value = pair.get(1)?.as_str()?.to_string();
                    Some((name, value))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn session_matches_filter(session: &JsonValue, filter: &str) -> bool {
    let filter = filter.trim().to_ascii_lowercase();
    if filter.is_empty() {
        return true;
    }
    ["id", "kind", "status", "method", "url", "error"]
        .iter()
        .filter_map(|key| session.get(*key))
        .any(|value| {
            value
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| value.to_string())
                .to_ascii_lowercase()
                .contains(&filter)
        })
}

pub(super) fn truncate(input: &str, max: usize) -> String {
    if input.chars().count() <= max {
        return input.to_string();
    }
    if max <= 3 {
        return ".".repeat(max);
    }
    let mut output = input.chars().take(max - 3).collect::<String>();
    output.push_str("...");
    output
}
