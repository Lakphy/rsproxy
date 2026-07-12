use super::transform::{apply_replace_transform, find_template_end};
use crate::{Captures, RequestMeta, ResolvedAction, ResponseMeta, UrlParts};
use std::cell::OnceCell;

impl ResolvedAction {
    pub fn new(action: crate::Action, rule: crate::MatchedRule, captures: Captures) -> Self {
        Self {
            action,
            rule,
            captures,
            response: None,
        }
    }

    pub fn render(&self, input: &str, request: &RequestMeta) -> String {
        self.captures
            .render_with_response(input, request, self.response.as_deref())
    }

    pub fn response_meta(&self) -> Option<&ResponseMeta> {
        self.response.as_deref()
    }
}

impl Captures {
    pub fn get_index(&self, index: usize) -> Option<&str> {
        if index == 0 {
            self.whole.as_deref()
        } else {
            self.indexed.get(index - 1).map(String::as_str)
        }
    }

    pub fn insert_index(&mut self, value: String) {
        if self.indexed.len() < 9 {
            self.indexed.push(value);
        }
    }

    pub fn render(&self, input: &str, request: &RequestMeta) -> String {
        self.render_with_response(input, request, None)
    }

    pub fn render_with_response(
        &self,
        input: &str,
        request: &RequestMeta,
        response: Option<&ResponseMeta>,
    ) -> String {
        let mut output = String::with_capacity(input.len());
        let url = OnceCell::new();
        let mut offset = 0;
        while let Some(relative) = input[offset..].find('$') {
            let start = offset + relative;
            output.push_str(&input[offset..start]);
            let tail = &input[start..];
            let bytes = tail.as_bytes();
            if bytes.len() >= 2 {
                if bytes[1].is_ascii_digit() {
                    let capture = (bytes[1] - b'0') as usize;
                    if let Some(value) = self.get_index(capture) {
                        output.push_str(value);
                    }
                    offset = start + 2;
                    continue;
                }
                if bytes[1] == b'{'
                    && let Some(end) = find_template_end(input, start + 2)
                {
                    output.push_str(&render_expression(
                        &input[start + 2..end],
                        request,
                        response,
                        self,
                        &url,
                    ));
                    offset = end + 1;
                    continue;
                }
            }
            output.push('$');
            offset = start + 1;
        }
        output.push_str(&input[offset..]);
        output
    }
}

fn render_expression(
    expression: &str,
    request: &RequestMeta,
    response: Option<&ResponseMeta>,
    captures: &Captures,
    url: &OnceCell<Option<UrlParts>>,
) -> String {
    if let Some(result) = apply_replace_transform(expression, |variable| {
        render_variable(variable, request, response, captures, url)
    }) {
        return result.unwrap_or_default();
    }
    render_variable(expression, request, response, captures, url)
}

fn render_variable(
    key: &str,
    request: &RequestMeta,
    response: Option<&ResponseMeta>,
    captures: &Captures,
    url: &OnceCell<Option<UrlParts>>,
) -> String {
    match key {
        "id" => request.template.id().to_string(),
        "now" => request.template.now_ms().to_string(),
        "random" => request.template.random().to_string(),
        "randomUUID" => request.template.random_uuid().to_string(),
        "url" => request.url.clone(),
        "method" => request.method.clone(),
        "host" | "hostname" => parsed_url(url, request)
            .map(|url| url.host.clone())
            .unwrap_or_default(),
        "port" => parsed_url(url, request)
            .and_then(UrlParts::effective_port)
            .map(|port| port.to_string())
            .unwrap_or_default(),
        "path" | "pathname" => parsed_url(url, request)
            .map(|url| url.path.clone())
            .unwrap_or_default(),
        "query" | "search" => parsed_url(url, request)
            .and_then(|url| url.query.clone())
            .unwrap_or_default(),
        "clientIp" => request.client_ip.clone().unwrap_or_default(),
        "serverIp" => request.server_ip.clone().unwrap_or_default(),
        "statusCode" => response
            .map(|response| response.status.to_string())
            .unwrap_or_default(),
        _ if key.starts_with("reqH.") => header(&request.headers, &key[5..]).unwrap_or_default(),
        _ if key.starts_with("resH.") => response
            .and_then(|response| header(&response.headers, &key[5..]))
            .unwrap_or_default(),
        _ if key.starts_with("reqCookies.") => {
            request_cookie(&request.headers, &key[11..]).unwrap_or_default()
        }
        _ if key.starts_with("resCookies.") => response
            .and_then(|response| response_cookie(&response.headers, &key[11..]))
            .unwrap_or_default(),
        _ => captures.named.get(key).cloned().unwrap_or_default(),
    }
}

fn parsed_url<'a>(
    url: &'a OnceCell<Option<UrlParts>>,
    request: &RequestMeta,
) -> Option<&'a UrlParts> {
    url.get_or_init(|| UrlParts::parse(&request.url).ok())
        .as_ref()
}

fn header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.clone())
}

fn request_cookie(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .filter(|(header_name, _)| header_name.eq_ignore_ascii_case("cookie"))
        .flat_map(|(_, value)| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find(|(cookie_name, _)| cookie_name.trim() == name)
        .map(|(_, value)| value.trim().to_string())
}

fn response_cookie(headers: &[(String, String)], name: &str) -> Option<String> {
    headers
        .iter()
        .filter(|(header_name, _)| header_name.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|(_, value)| value.split(';').next())
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find(|(cookie_name, _)| cookie_name.trim() == name)
        .map(|(_, value)| value.trim().to_string())
}
