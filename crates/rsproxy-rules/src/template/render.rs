use super::transform::{apply_replace_transform_bounded, find_template_end};
use crate::matching::header;
use crate::{Captures, RequestMeta, ResolvedAction, ResponseMeta, UrlParts};
use std::borrow::Cow;
use std::cell::OnceCell;
use std::sync::Arc;

impl ResolvedAction {
    /// Associates a programmatic action with provenance and matcher captures.
    ///
    /// The resulting action has no response snapshot; response-only template
    /// variables render empty unless the value came from response resolution.
    pub fn new(action: crate::Action, rule: crate::MatchedRule, captures: Captures) -> Self {
        Self {
            action,
            rule,
            captures,
            response: None,
        }
    }

    /// Renders captures and request/response variables using this action's match context.
    pub fn render(&self, input: &str, request: &RequestMeta) -> String {
        self.captures
            .render_with_response(input, request, self.response.as_deref())
    }

    /// Renders this action's captures and variables within a byte limit.
    pub fn render_bounded(
        &self,
        input: &str,
        request: &RequestMeta,
        limit: usize,
    ) -> Result<String, crate::RuleModelError> {
        self.captures
            .render_with_response_bounded(input, request, self.response.as_deref(), limit)
    }

    /// Borrows the response snapshot captured by response-phase resolution, if any.
    pub fn response_meta(&self) -> Option<&ResponseMeta> {
        self.response.as_deref()
    }
}

impl Captures {
    /// Returns `$0` for the complete match or `$1` through `$9` for capture groups.
    pub fn get_index(&self, index: usize) -> Option<&str> {
        if index == 0 {
            self.whole.as_deref()
        } else {
            self.indexed.get(index - 1).map(AsRef::as_ref)
        }
    }

    /// Appends a numbered capture while enforcing the public `$1`–`$9` limit.
    pub fn insert_index(&mut self, value: String) {
        if self.indexed.len() < 9 {
            self.indexed.push(Arc::from(value));
        }
    }

    /// Renders captures and request variables; response variables become empty.
    pub fn render(&self, input: &str, request: &RequestMeta) -> String {
        self.render_with_response(input, request, None)
    }

    /// Renders request variables and captures within a byte limit.
    pub fn render_bounded(
        &self,
        input: &str,
        request: &RequestMeta,
        limit: usize,
    ) -> Result<String, crate::RuleModelError> {
        self.render_with_response_bounded(input, request, None, limit)
    }

    /// Renders captures plus request and optional response template variables.
    ///
    /// Header lookup is case-insensitive, cookie names are case-sensitive, and
    /// malformed programmatic placeholders are preserved as literal text.
    pub fn render_with_response(
        &self,
        input: &str,
        request: &RequestMeta,
        response: Option<&ResponseMeta>,
    ) -> String {
        self.render_with_response_bounded(input, request, response, usize::MAX)
            .expect("rendering without a byte limit cannot exceed it")
    }

    /// Renders request/response variables and captures without exceeding `limit` bytes.
    pub fn render_with_response_bounded(
        &self,
        input: &str,
        request: &RequestMeta,
        response: Option<&ResponseMeta>,
        limit: usize,
    ) -> Result<String, crate::RuleModelError> {
        let mut output = String::with_capacity(input.len().min(limit));
        let url = OnceCell::new();
        let mut offset = 0;
        while let Some(relative) = input[offset..].find('$') {
            let start = offset + relative;
            push_bounded(&mut output, &input[offset..start], limit)?;
            let tail = &input[start..];
            let bytes = tail.as_bytes();
            if bytes.len() >= 2 {
                if bytes[1].is_ascii_digit() {
                    let capture = (bytes[1] - b'0') as usize;
                    if let Some(value) = self.get_index(capture) {
                        push_bounded(&mut output, value, limit)?;
                    }
                    offset = start + 2;
                    continue;
                }
                if bytes[1] == b'{'
                    && let Some(end) = find_template_end(input, start + 2)
                {
                    let remaining = limit.saturating_sub(output.len());
                    let value = render_expression_bounded(
                        &input[start + 2..end],
                        request,
                        response,
                        self,
                        &url,
                        remaining,
                    )?;
                    push_bounded(&mut output, &value, limit)?;
                    offset = end + 1;
                    continue;
                }
            }
            push_bounded(&mut output, "$", limit)?;
            offset = start + 1;
        }
        push_bounded(&mut output, &input[offset..], limit)?;
        Ok(output)
    }
}

fn render_expression_bounded<'a>(
    expression: &str,
    request: &'a RequestMeta,
    response: Option<&'a ResponseMeta>,
    captures: &'a Captures,
    url: &'a OnceCell<Option<UrlParts>>,
    limit: usize,
) -> Result<Cow<'a, str>, crate::RuleModelError> {
    if let Some(result) = apply_replace_transform_bounded(
        expression,
        |variable| render_variable(variable, request, response, captures, url).into_owned(),
        limit,
    ) {
        return match result {
            Ok(value) => Ok(Cow::Owned(value)),
            Err(error @ crate::RuleModelError::LimitExceeded { .. }) => Err(error),
            Err(_) => Ok(Cow::Borrowed("")),
        };
    }
    let value = render_variable(expression, request, response, captures, url);
    if value.len() > limit {
        return Err(render_limit(limit));
    }
    Ok(value)
}

fn push_bounded(
    output: &mut String,
    value: &str,
    limit: usize,
) -> Result<(), crate::RuleModelError> {
    crate::bounded_replace::push_bounded(output, value, limit, RENDER_CONTEXT)
}

fn render_limit(limit: usize) -> crate::RuleModelError {
    crate::bounded_replace::limit_error(RENDER_CONTEXT, limit)
}

const RENDER_CONTEXT: &str = "template rendering";

fn render_variable<'a>(
    key: &str,
    request: &'a RequestMeta,
    response: Option<&'a ResponseMeta>,
    captures: &'a Captures,
    url: &'a OnceCell<Option<UrlParts>>,
) -> Cow<'a, str> {
    match key {
        "id" => Cow::Borrowed(request.template.id()),
        "now" => Cow::Owned(request.template.now_ms().to_string()),
        "random" => Cow::Owned(request.template.random().to_string()),
        "randomUUID" => Cow::Borrowed(request.template.random_uuid()),
        "url" => Cow::Borrowed(&request.url),
        "method" => Cow::Borrowed(&request.method),
        "host" | "hostname" => parsed_url(url, request)
            .map(|url| Cow::Borrowed(url.host.as_str()))
            .unwrap_or_default(),
        "port" => parsed_url(url, request)
            .and_then(UrlParts::effective_port)
            .map(|port| Cow::Owned(port.to_string()))
            .unwrap_or_default(),
        "path" | "pathname" => parsed_url(url, request)
            .map(|url| Cow::Borrowed(url.path.as_str()))
            .unwrap_or_default(),
        "query" | "search" => parsed_url(url, request)
            .and_then(|url| url.query.as_deref())
            .map(Cow::Borrowed)
            .unwrap_or_default(),
        "clientIp" => request
            .client_ip
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_default(),
        "serverIp" => request
            .server_ip
            .as_deref()
            .map(Cow::Borrowed)
            .unwrap_or_default(),
        "statusCode" => response
            .map(|response| Cow::Owned(response.status.to_string()))
            .unwrap_or_default(),
        _ if key.starts_with("reqH.") => header(&request.headers, &key[5..])
            .map(Cow::Borrowed)
            .unwrap_or_default(),
        _ if key.starts_with("resH.") => response
            .and_then(|response| header(&response.headers, &key[5..]))
            .map(Cow::Borrowed)
            .unwrap_or_default(),
        _ if key.starts_with("reqCookies.") => request_cookie(&request.headers, &key[11..])
            .map(Cow::Borrowed)
            .unwrap_or_default(),
        _ if key.starts_with("resCookies.") => response
            .and_then(|response| response_cookie(&response.headers, &key[11..]))
            .map(Cow::Borrowed)
            .unwrap_or_default(),
        _ => captures
            .named
            .get(key)
            .map(|value| Cow::Borrowed(value.as_ref()))
            .unwrap_or_default(),
    }
}

fn parsed_url<'a>(
    url: &'a OnceCell<Option<UrlParts>>,
    request: &RequestMeta,
) -> Option<&'a UrlParts> {
    url.get_or_init(|| UrlParts::parse(&request.url).ok())
        .as_ref()
}

fn request_cookie<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .filter(|(header_name, _)| header_name.eq_ignore_ascii_case("cookie"))
        .flat_map(|(_, value)| value.split(';'))
        .filter_map(|part| part.trim().split_once('='))
        .find(|(cookie_name, _)| cookie_name.trim() == name)
        .map(|(_, value)| value.trim())
}

fn response_cookie<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .filter(|(header_name, _)| header_name.eq_ignore_ascii_case("set-cookie"))
        .filter_map(|(_, value)| value.split(';').next())
        .filter_map(|cookie| cookie.trim().split_once('='))
        .find(|(cookie_name, _)| cookie_name.trim() == name)
        .map(|(_, value)| value.trim())
}
