use super::*;

pub(super) fn run_rules_test(args: &[String], api: &str, storage: &Path) -> Result<(), String> {
    let url = request_url(args).ok_or_else(|| "rules test requires URL".to_string())?;
    let method = request_method(args);
    let headers = request_headers(args)?;
    let body = request_body(args);
    let client_ip = request_client_ip(args);
    let server_ip = request_server_ip(args, &url);
    let request = RequestMeta {
        method,
        url,
        headers,
        body,
        client_ip,
        server_ip,
        template: Default::default(),
    };
    let response = response_meta(args)?;
    let path = rules_test_api_path(&request, response.as_ref());
    let explain = match api_request("GET", api, &path, "") {
        Ok(body) => body,
        Err(_) => {
            let rules = crate::rule_store::RuleStore::load(storage)
                .map_err(|error| error.to_string())?
                .snapshot()
                .compiled
                .clone();
            match &response {
                Some(response) => rules.explain_response(&request, response),
                None => rules.explain(&request),
            }
        }
    };
    if has_flag(args, "--json") {
        println!(
            "{}",
            serde_json::json!({
                "url": request.url,
                "phase": if response.is_some() { "response" } else { "request" },
                "explain": explain,
            })
        );
    } else {
        print!("{explain}");
    }
    Ok(())
}

pub(in crate::cli) fn request_url(args: &[String]) -> Option<String> {
    positional_skipping_values(
        args,
        &[
            "--api",
            "--api-token",
            "--config",
            "--storage",
            "--url",
            "--method",
            "-X",
            "--header",
            "-H",
            "--client-ip",
            "--server-ip",
            "--response-status",
            "--response-header",
            "--body",
            "-d",
            "--iterations",
            "-n",
            "--warmup",
        ],
    )
}

pub(in crate::cli) fn request_method(args: &[String]) -> String {
    option_value(args, "-X")
        .or_else(|| option_value(args, "--method"))
        .unwrap_or_else(|| "GET".to_string())
}

pub(in crate::cli) fn request_headers(args: &[String]) -> Result<Vec<(String, String)>, String> {
    option_values(args, &["-H", "--header"])
        .into_iter()
        .map(|value| parse_header_arg(&value))
        .collect()
}

pub(in crate::cli) fn request_client_ip(args: &[String]) -> Option<String> {
    option_value(args, "--client-ip").filter(|value| !value.trim().is_empty())
}

pub(in crate::cli) fn request_server_ip(args: &[String], url: &str) -> Option<String> {
    option_value(args, "--server-ip")
        .filter(|value| !value.trim().is_empty())
        .or_else(|| literal_ip_from_url(url))
}

pub(in crate::cli) fn request_body(args: &[String]) -> Vec<u8> {
    option_value(args, "--body")
        .or_else(|| option_value(args, "-d"))
        .unwrap_or_default()
        .into_bytes()
}

pub(in crate::cli) fn response_meta(args: &[String]) -> Result<Option<ResponseMeta>, String> {
    let status = option_value(args, "--response-status")
        .map(|value| parse_response_status(&value))
        .transpose()?;
    let headers = option_values(args, &["--response-header"])
        .into_iter()
        .map(|value| parse_header_arg(&value))
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

fn parse_response_status(value: &str) -> Result<u16, String> {
    let status = value
        .parse::<u16>()
        .map_err(|_| format!("response status must be numeric: `{value}`"))?;
    if !(100..=599).contains(&status) {
        return Err(format!(
            "response status must be between 100 and 599: `{value}`"
        ));
    }
    Ok(status)
}

pub(in crate::cli) fn parse_header_arg(value: &str) -> Result<(String, String), String> {
    let (name, value) = value
        .split_once(':')
        .ok_or_else(|| format!("header must use `Name: value` syntax: `{value}`"))?;
    let name = name.trim();
    if name.is_empty() {
        return Err("header name must not be empty".to_string());
    }
    if !valid_header_name(name) {
        return Err(format!("invalid header name `{name}`"));
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

fn literal_ip_from_url(url: &str) -> Option<String> {
    let host = UrlParts::parse(url).ok()?.host;
    host.parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| ip.to_string())
}
