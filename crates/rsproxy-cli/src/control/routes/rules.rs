use super::respond_json;
use crate::app::SharedState;
use crate::control::query::{
    literal_ip_from_url, parse_header_query_value, query_get, query_get_all,
};
use crate::rule_store::RuleStoreError;
use crate::{http, json};
use rsproxy_rules::{RequestMeta, ResponseMeta, RuleError, RuleSet};
use std::io::Write;

const GROUP_PREFIX: &str = "/api/rules/";

pub(super) fn list<W: Write + ?Sized>(stream: &mut W, state: &SharedState) -> std::io::Result<()> {
    let snapshot = state.rules.snapshot();
    let groups = snapshot
        .groups
        .iter()
        .enumerate()
        .map(|(order, group)| {
            let rules = RuleSet::parse(&group.name, &group.text)
                .map(|rules| rules.rules.len())
                .unwrap_or_default();
            format!(
                "{{\"name\":{},\"enabled\":{},\"order\":{},\"rules\":{}}}",
                json::string(&group.name),
                group.enabled,
                order,
                rules
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    respond_json(stream, 200, &format!("[{groups}]"))
}

pub(super) fn export<W: Write + ?Sized>(
    stream: &mut W,
    state: &SharedState,
) -> std::io::Result<()> {
    let snapshot = state.rules.snapshot();
    let groups = snapshot
        .groups
        .iter()
        .map(|group| {
            format!(
                "{{\"name\":{},\"enabled\":{},\"text\":{}}}",
                json::string(&group.name),
                group.enabled,
                json::string(&group.text)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    respond_json(stream, 200, &format!("[{groups}]"))
}

pub(super) fn group<W: Write + ?Sized>(
    stream: &mut W,
    state: &SharedState,
    method: &str,
    path: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let Some(route) = group_route(path) else {
        return respond_json(stream, 404, "{\"error\":\"not found\"}");
    };
    match (method, route.action) {
        ("GET", None) => get_group(stream, state, route.name),
        ("POST" | "PUT", None) => set_group(stream, state, route.name, body),
        ("DELETE", None) => change_group(stream, state.rules.remove_group(route.name)),
        ("POST", Some("enable")) => change_group(stream, state.rules.set_enabled(route.name, true)),
        ("POST", Some("disable")) => {
            change_group(stream, state.rules.set_enabled(route.name, false))
        }
        _ => respond_json(stream, 404, "{\"error\":\"not found\"}"),
    }
}

pub(super) fn check<W: Write + ?Sized>(stream: &mut W, body: &[u8]) -> std::io::Result<()> {
    let text = String::from_utf8_lossy(body);
    match RuleSet::parse("default", &text) {
        Ok(rules) => respond_json(
            stream,
            200,
            &format!("{{\"ok\":true,\"rules\":{}}}", rules.rules.len()),
        ),
        Err(errors) => parse_errors(stream, &errors),
    }
}

pub(super) fn test<W: Write + ?Sized>(
    stream: &mut W,
    state: &SharedState,
    query: Option<&str>,
) -> std::io::Result<()> {
    let url = query_get(query, "url").unwrap_or_default();
    let method = query_get(query, "method").unwrap_or_else(|| "GET".to_string());
    let headers = query_get_all(query, "header")
        .into_iter()
        .filter_map(|value| parse_header_query_value(&value))
        .collect();
    let body = query_get(query, "body").unwrap_or_default().into_bytes();
    let client_ip = query_get(query, "clientIp").filter(|value| !value.trim().is_empty());
    let server_ip = query_get(query, "serverIp")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| literal_ip_from_url(&url));
    let request = RequestMeta {
        method,
        url,
        headers,
        body,
        client_ip,
        server_ip,
        template: Default::default(),
    };
    let response_status = match query_get(query, "responseStatus") {
        Some(value) => match value.parse::<u16>() {
            Ok(status) if (100..=599).contains(&status) => Some(status),
            _ => {
                return respond_json(
                    stream,
                    400,
                    "{\"error\":\"responseStatus must be between 100 and 599\"}",
                );
            }
        },
        None => None,
    };
    let response_header_values = query_get_all(query, "responseHeader");
    let Some(response_headers) = response_header_values
        .iter()
        .map(|value| parse_header_query_value(value))
        .collect::<Option<Vec<_>>>()
    else {
        return respond_json(
            stream,
            400,
            "{\"error\":\"responseHeader must use Name: value syntax\"}",
        );
    };
    let response = if response_status.is_some() || !response_header_values.is_empty() {
        Some(ResponseMeta {
            status: response_status.unwrap_or(200),
            headers: response_headers,
        })
    } else {
        None
    };
    let snapshot = state.rules.snapshot();
    let body = match &response {
        Some(response) => snapshot.compiled.explain_response(&request, response),
        None => snapshot.compiled.explain(&request),
    };
    http::write_response(
        stream,
        200,
        "OK",
        &[(
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        )],
        body.as_bytes(),
    )
}

fn get_group<W: Write + ?Sized>(
    stream: &mut W,
    state: &SharedState,
    name: &str,
) -> std::io::Result<()> {
    let snapshot = state.rules.snapshot();
    let Some(group) = snapshot.group(name) else {
        return respond_json(
            stream,
            404,
            &format!("{{\"error\":{}}}", json::string("rule group not found")),
        );
    };
    http::write_response(
        stream,
        200,
        "OK",
        &[(
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        )],
        group.text.as_bytes(),
    )
}

fn set_group<W: Write + ?Sized>(
    stream: &mut W,
    state: &SharedState,
    name: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let text = match std::str::from_utf8(body) {
        Ok(text) => text.to_string(),
        Err(_) => {
            return respond_json(stream, 400, "{\"error\":\"rule text must be valid UTF-8\"}");
        }
    };
    change_group(stream, state.rules.set_group(name, text))
}

fn change_group<W: Write + ?Sized>(
    stream: &mut W,
    result: Result<std::sync::Arc<crate::rule_store::RuleSnapshot>, RuleStoreError>,
) -> std::io::Result<()> {
    match result {
        Ok(snapshot) => respond_json(
            stream,
            200,
            &format!(
                "{{\"ok\":true,\"groups\":{},\"rules\":{}}}",
                snapshot.groups.len(),
                snapshot.compiled.rules.len()
            ),
        ),
        Err(RuleStoreError::Parse(errors)) => parse_errors(stream, &errors),
        Err(error) => respond_json(
            stream,
            error.http_status(),
            &format!("{{\"error\":{}}}", json::string(&error.to_string())),
        ),
    }
}

fn parse_errors<W: Write + ?Sized>(stream: &mut W, errors: &[RuleError]) -> std::io::Result<()> {
    respond_json(
        stream,
        400,
        &format!(
            "{{\"ok\":false,\"errors\":[{}]}}",
            errors
                .iter()
                .map(|error| format!(
                    "{{\"code\":{},\"group\":{},\"line\":{},\"message\":{}}}",
                    json::string(error.code.as_str()),
                    json::string(&error.group),
                    error.line,
                    json::string(&error.message)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
    )
}

struct GroupRoute<'a> {
    name: &'a str,
    action: Option<&'a str>,
}

fn group_route(path: &str) -> Option<GroupRoute<'_>> {
    let suffix = path.strip_prefix(GROUP_PREFIX)?;
    let mut parts = suffix.split('/');
    let name = parts.next().filter(|name| !name.is_empty())?;
    let action = parts.next();
    if parts.next().is_some() || action.is_some_and(|value| !matches!(value, "enable" | "disable"))
    {
        return None;
    }
    Some(GroupRoute { name, action })
}
