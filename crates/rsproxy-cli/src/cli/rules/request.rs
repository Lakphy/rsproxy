use crate::cli::command::{RequestArgs, RulesTestArgs};
use crate::cli::util::percent_encode;
use crate::{CliError, CliResult};
use rsproxy_control::api_request;
use rsproxy_rules::{RequestMeta, ResponseMeta, UrlParts};
use std::path::Path;

use super::advisory::{print_advisories, request_advisories};
use super::groups::load_rule_set;

pub(super) fn run_rules_test(
    args: RulesTestArgs,
    json: bool,
    api: &str,
    storage: &Path,
    no_mitm: bool,
) -> CliResult<()> {
    let request = request_meta(&args.request, args.url)?;
    let response = response_meta(args.response_status.as_deref(), &args.response_header)?;
    let path = rules_test_api_path(&request, response.as_ref());
    let explain = match api_request("GET", api, &path, "") {
        Ok(body) => body,
        Err(_) => {
            let rules = rsproxy_engine::RuleStore::load(storage)?
                .snapshot()
                .compiled
                .clone();
            match &response {
                Some(response) => rules.explain_response(&request, response),
                None => rules.explain(&request),
            }
        }
    };
    let rules = load_rule_set(None, api, storage)?;
    let advisories = request_advisories(&rules, &request, storage, no_mitm);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "url": request.url,
                "phase": if response.is_some() { "response" } else { "request" },
                "explain": explain,
                "warnings": advisories
                    .iter()
                    .map(super::advisory::EnvironmentAdvisory::to_json)
                    .collect::<Vec<_>>(),
            })
        );
    } else {
        print!("{explain}");
        if !advisories.is_empty() {
            if !explain.ends_with('\n') {
                println!();
            }
            print_advisories(&advisories);
        }
    }
    Ok(())
}

pub(in crate::cli) fn request_meta(args: &RequestArgs, url: String) -> CliResult<RequestMeta> {
    Ok(RequestMeta {
        method: args.method.clone(),
        headers: request_headers(args)?,
        body: args.body.clone().unwrap_or_default().into_bytes(),
        client_ip: nonempty(args.client_ip.as_deref()),
        server_ip: nonempty(args.server_ip.as_deref()).or_else(|| literal_ip_from_url(&url)),
        url,
        template: Default::default(),
    })
}

pub(super) fn request_headers(args: &RequestArgs) -> CliResult<Vec<(String, String)>> {
    args.header
        .iter()
        .map(|value| parse_header_arg(value))
        .collect()
}

pub(in crate::cli) fn response_meta(
    status: Option<&str>,
    header_values: &[String],
) -> CliResult<Option<ResponseMeta>> {
    let status = status.map(parse_response_status).transpose()?;
    let headers = header_values
        .iter()
        .map(|value| parse_header_arg(value))
        .collect::<Result<Vec<_>, _>>()?;
    if status.is_none() && headers.is_empty() {
        Ok(None)
    } else {
        Ok(Some(ResponseMeta {
            status: status.unwrap_or(200),
            headers,
        }))
    }
}

fn parse_response_status(value: &str) -> CliResult<u16> {
    let status = value
        .parse::<u16>()
        .map_err(|_| CliError::Usage(format!("response status must be numeric: `{value}`")))?;
    if !(100..=599).contains(&status) {
        return Err(CliError::Usage(format!(
            "response status must be between 100 and 599: `{value}`"
        )));
    }
    Ok(status)
}

pub(in crate::cli) fn parse_header_arg(value: &str) -> CliResult<(String, String)> {
    let (name, value) = value.split_once(':').ok_or_else(|| {
        CliError::Usage(format!("header must use `Name: value` syntax: `{value}`"))
    })?;
    let name = name.trim();
    if name.is_empty() {
        return Err(CliError::Usage("header name must not be empty".to_string()));
    }
    if !valid_header_name(name) {
        return Err(CliError::Usage(format!("invalid header name `{name}`")));
    }
    Ok((name.to_string(), value.trim_start().to_string()))
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

pub(in crate::cli) fn rules_test_api_path(
    request: &RequestMeta,
    response: Option<&ResponseMeta>,
) -> String {
    let mut path = format!(
        "/api/rules/test?url={}&method={}",
        percent_encode(&request.url),
        percent_encode(&request.method)
    );
    for (name, value) in &request.headers {
        path.push_str("&header=");
        path.push_str(&percent_encode(&format!("{name}: {value}")));
    }
    if !request.body.is_empty() {
        path.push_str("&body=");
        path.push_str(&percent_encode(&String::from_utf8_lossy(&request.body)));
    }
    if let Some(client_ip) = &request.client_ip {
        path.push_str("&clientIp=");
        path.push_str(&percent_encode(client_ip));
    }
    if let Some(server_ip) = &request.server_ip {
        path.push_str("&serverIp=");
        path.push_str(&percent_encode(server_ip));
    }
    if let Some(response) = response {
        path.push_str("&responseStatus=");
        path.push_str(&response.status.to_string());
        for (name, value) in &response.headers {
            path.push_str("&responseHeader=");
            path.push_str(&percent_encode(&format!("{name}: {value}")));
        }
    }
    path
}

fn nonempty(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn literal_ip_from_url(url: &str) -> Option<String> {
    let host = UrlParts::parse(url).ok()?.host;
    host.parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| ip.to_string())
}
