use rsproxy_rules::{RequestMeta, ResponseMeta, RuleSet, UrlParts, redact_secrets};

const MAX_INPUT: usize = 64 * 1024;
const INPUT_SEPARATOR: &str = "\n---request-url---\n";

pub(super) fn exercise(data: &[u8]) {
    if data.len() > MAX_INPUT {
        return;
    }
    let text = String::from_utf8_lossy(data);
    let (source, candidate_url) = text
        .split_once(INPUT_SEPARATOR)
        .map(|(source, url)| (source, Some(url)))
        .unwrap_or((&text, None));
    let _ = redact_secrets(source);
    let Ok(rules) = RuleSet::parse("fuzz", source) else {
        return;
    };

    let response = ResponseMeta {
        status: 100 + data.first().copied().unwrap_or_default() as u16 % 500,
        headers: vec![("X-Fuzz-Response".to_string(), prefix(&text, 128))],
    };
    for url in [
        "http://example.test/api?mode=fuzz".to_string(),
        candidate_url.unwrap_or(&text).to_string(),
    ] {
        let request = RequestMeta {
            method: if data.len().is_multiple_of(2) {
                "GET".to_string()
            } else {
                "POST".to_string()
            },
            url,
            headers: vec![("X-Fuzz".to_string(), prefix(&text, 128))],
            body: data[..data.len().min(4096)].to_vec(),
            client_ip: Some("192.0.2.10".to_string()),
            server_ip: Some("198.51.100.20".to_string()),
            template: Default::default(),
        };
        let _ = UrlParts::parse(&request.url);
        let _ = rules.stats();
        let _ = rules.request_body_required(&request);
        let _ = rules.resolve(&request);
        let _ = rules.resolve_without_request_body(&request);
        let _ = rules.resolve_response(&request, &response);
        let _ = rules.resolve_response_without_request_body(&request, &response);
        let _ = rules.explain(&request);
        let _ = rules.explain_response(&request, &response);
    }
}

fn prefix(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}
