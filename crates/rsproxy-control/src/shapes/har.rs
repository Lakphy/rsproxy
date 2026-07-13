use super::kind;
use rsproxy_trace::{Session, SessionKind, TlsRecord};
use serde_json::{Value as JsonValue, json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) fn sessions_har(sessions: &[Session]) -> String {
    let entries = sessions
        .iter()
        .filter(|session| {
            matches!(
                session.kind,
                SessionKind::Http | SessionKind::Sse | SessionKind::WebSocket
            )
        })
        .map(har_entry)
        .collect::<Vec<_>>();
    serde_json::to_string(&json!({
        "log": {
            "version": "1.2",
            "creator": {
                "name": "rsproxy",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "entries": entries,
        }
    }))
    .expect("HAR values are serializable")
}

fn har_entry(session: &Session) -> JsonValue {
    let upstream_tls_ms = tls_duration_ms(session, "upstream_tls");
    let client_tls_ms = tls_duration_ms(session, "client_mitm_tls");
    let recorded_tls_ms = session.tls.iter().fold(0u64, |total, record| {
        total.saturating_add(record.handshake_ms)
    });
    let h2_client = session.flags.iter().any(|flag| flag == "h2-client");
    let client_tls_in_timeline = !h2_client;
    let timeline_tls_ms = upstream_tls_ms.saturating_add(if client_tls_in_timeline {
        client_tls_ms
    } else {
        0
    });
    let request_send_ms = session.request_send_ms.unwrap_or(0);
    let legacy_receive_ms = session
        .duration_ms
        .saturating_sub(session.pool_wait_ms)
        .saturating_sub(session.dns_ms)
        .saturating_sub(session.connect_ms)
        .saturating_sub(upstream_tls_ms)
        .saturating_sub(request_send_ms)
        .saturating_sub(session.ttfb_ms);
    let response_receive_ms = session.response_receive_ms.unwrap_or(legacy_receive_ms);
    let standard_response_receive_ms = response_receive_ms.min(legacy_receive_ms);
    let standard_unattributed_ms = if session.response_receive_ms.is_some() {
        legacy_receive_ms.saturating_sub(standard_response_receive_ms)
    } else {
        0
    };
    let unattributed_before_receive = session
        .duration_ms
        .saturating_sub(session.pool_wait_ms)
        .saturating_sub(session.dns_ms)
        .saturating_sub(session.connect_ms)
        .saturating_sub(timeline_tls_ms)
        .saturating_sub(request_send_ms)
        .saturating_sub(session.ttfb_ms);
    let unattributed_ms = session
        .response_receive_ms
        .map(|receive_ms| unattributed_before_receive.saturating_sub(receive_ms))
        .unwrap_or(unattributed_before_receive);
    let transfer_overlap_ms = session
        .response_receive_ms
        .map(|receive_ms| receive_ms.saturating_sub(unattributed_before_receive))
        .unwrap_or(0);
    let blocked_ms = session
        .pool_wait_ms
        .saturating_add(standard_unattributed_ms);
    let ssl = if upstream_tls_ms == 0 {
        JsonValue::from(-1)
    } else {
        JsonValue::from(upstream_tls_ms)
    };
    let http_version = if h2_client { "HTTP/2" } else { "HTTP/1.1" };

    json!({
        "startedDateTime": har_started_datetime(session.started_ms),
        "time": session.duration_ms,
        "request": {
            "method": session.method,
            "url": session.url,
            "httpVersion": http_version,
            "headers": har_headers(&session.req_headers),
            "queryString": har_query_string(&session.url),
            "headersSize": -1,
            "bodySize": session.request_bytes,
            "_trailers": har_headers(&session.req_trailers),
        },
        "response": {
            "status": session.status.unwrap_or(0),
            "statusText": "",
            "httpVersion": http_version,
            "headers": har_headers(&session.res_headers),
            "content": {
                "size": session.response_bytes,
                "mimeType": content_type(&session.res_headers),
                "text": String::from_utf8_lossy(&session.res_body_head),
            },
            "redirectURL": "",
            "headersSize": -1,
            "bodySize": session.response_bytes,
            "_trailers": har_headers(&session.res_trailers),
        },
        "cache": {},
        "timings": {
            "blocked": blocked_ms,
            "dns": session.dns_ms,
            "connect": session.connect_ms,
            "ssl": ssl,
            "send": request_send_ms,
            "wait": session.ttfb_ms,
            "receive": standard_response_receive_ms,
        },
        "_rsproxy": {
            "session_id": session.id,
            "kind": kind(session.kind),
            "client": session.client,
            "upstream": session.upstream,
            "flags": session.flags,
            "error": session.error,
            "rules": session.matched_rules.iter().map(|rule| json!({
                "group": rule.group,
                "line": rule.line,
                "raw": rsproxy_rules::redact_secrets(&rule.raw),
            })).collect::<Vec<_>>(),
            "tls": har_tls_records(&session.tls),
            "frame_count": session.frames.len(),
            "timings": {
                "pool_wait_ms": session.pool_wait_ms,
                "dns_ms": session.dns_ms,
                "connect_ms": session.connect_ms,
                "client_tls_ms": client_tls_ms,
                "client_tls_in_timeline": client_tls_in_timeline,
                "upstream_tls_ms": upstream_tls_ms,
                "recorded_tls_ms": recorded_tls_ms,
                "timeline_tls_ms": timeline_tls_ms,
                "request_send_ms": session.request_send_ms,
                "ttfb_ms": session.ttfb_ms,
                "response_receive_ms": session.response_receive_ms,
                "transfer_overlap_ms": transfer_overlap_ms,
                "boundaries_complete": session.request_send_ms.is_some()
                    && session.response_receive_ms.is_some(),
                "unattributed_ms": unattributed_ms,
            },
        },
    })
}

fn har_headers(headers: &[(String, String)]) -> JsonValue {
    JsonValue::Array(
        headers
            .iter()
            .map(|(name, value)| json!({"name": name, "value": value}))
            .collect(),
    )
}

fn har_query_string(url: &str) -> Vec<JsonValue> {
    let Some(query) = rsproxy_rules::UrlParts::parse(url)
        .ok()
        .and_then(|parts| parts.query)
    else {
        return Vec::new();
    };
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            json!({
                "name": percent_decode_query(name),
                "value": percent_decode_query(value),
            })
        })
        .collect()
}

fn percent_decode_query(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                decoded.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                match (hex_value(bytes[index + 1]), hex_value(bytes[index + 2])) {
                    (Some(high), Some(low)) => {
                        decoded.push((high << 4) | low);
                        index += 3;
                    }
                    _ => {
                        decoded.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn har_tls_records(records: &[TlsRecord]) -> Vec<JsonValue> {
    records
        .iter()
        .map(|record| {
            json!({
                "phase": record.phase,
                "host": record.host,
                "handshake_ms": record.handshake_ms,
                "peer_certificates": record.peer_certificates,
                "protocol": record.protocol,
                "cipher_suite": record.cipher_suite,
                "alpn": record.alpn,
                "error": record.error,
            })
        })
        .collect()
}

fn tls_duration_ms(session: &Session, phase: &str) -> u64 {
    session
        .tls
        .iter()
        .filter(|record| record.phase == phase)
        .fold(0u64, |total, record| {
            total.saturating_add(record.handshake_ms)
        })
}

fn har_started_datetime(started_ms: u64) -> String {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(started_ms) * 1_000_000)
        .unwrap_or(OffsetDateTime::UNIX_EPOCH)
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

fn content_type(headers: &[(String, String)]) -> &str {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.as_str())
        .unwrap_or("application/octet-stream")
}
