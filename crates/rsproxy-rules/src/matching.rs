use super::*;

pub(super) fn split_host_port(authority: &str) -> (&str, Option<u16>) {
    if authority.starts_with('[')
        && let Some(end) = authority.find(']')
    {
        let host = &authority[..=end];
        let port = authority[end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok());
        return (host, port);
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => (host, port.parse::<u16>().ok()),
        _ => (authority, None),
    }
}

pub(super) fn exact_url_matches(expected: &str, url: &UrlParts) -> bool {
    let Ok(expected) = UrlParts::parse(expected) else {
        return false;
    };
    expected.scheme == url.scheme
        && expected.host == url.host
        && expected.effective_port() == url.effective_port()
        && expected.path == url.path
        && (expected.query.is_none() || expected.query == url.query)
}

pub(super) fn host_matches(pattern: &str, host: &str) -> bool {
    let pattern = pattern.trim_matches(['[', ']']).to_ascii_lowercase();
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if pattern == "**" || pattern == "*" {
        return true;
    }
    if let Some(base) = pattern.strip_prefix("**.") {
        return host == base || host.ends_with(&format!(".{base}"));
    }
    if let Some(base) = pattern.strip_prefix("*.") {
        if !host.ends_with(&format!(".{base}")) {
            return false;
        }
        let prefix = &host[..host.len() - base.len() - 1];
        return !prefix.is_empty() && !prefix.contains('.');
    }
    if pattern.contains('*') {
        glob_match(&pattern, &host, '.')
    } else {
        pattern == host
    }
}

pub(super) fn normalize_ip_value(value: &str) -> String {
    value
        .parse::<SocketAddr>()
        .map(|addr| addr.ip().to_string())
        .unwrap_or_else(|_| value.trim().trim_matches(['[', ']']).to_string())
}

pub(super) fn ip_matches(pattern: &str, actual: &str) -> bool {
    let pattern = normalize_ip_value(pattern);
    let actual = normalize_ip_value(actual);
    if pattern == "*" || pattern == "**" {
        true
    } else if pattern.contains('*') {
        glob_match(&pattern, &actual, '\0')
    } else {
        pattern.eq_ignore_ascii_case(&actual)
    }
}

pub(super) fn path_prefix_matches(pattern: &str, path: &str) -> bool {
    if pattern == "/" {
        return true;
    }
    path == pattern
        || (path.starts_with(pattern)
            && (pattern.ends_with('/') || path.as_bytes().get(pattern.len()) == Some(&b'/')))
}

pub(super) fn glob_match(pattern: &str, text: &str, sep: char) -> bool {
    let mut captures = Captures::default();
    glob_match_with_captures(pattern, text, sep, &mut captures)
}

pub(super) fn glob_match_with_captures(
    pattern: &str,
    text: &str,
    sep: char,
    captures: &mut Captures,
) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let before = captures.indexed.len();
    let matched = glob_match_rec(&p, 0, &t, 0, sep, captures);
    if !matched {
        captures.indexed.truncate(before);
    }
    matched
}

pub(super) fn glob_match_rec(
    pattern: &[char],
    pi: usize,
    text: &[char],
    ti: usize,
    sep: char,
    captures: &mut Captures,
) -> bool {
    if pi == pattern.len() {
        return ti == text.len();
    }
    if pattern[pi] == '\\' {
        return pi + 1 < pattern.len()
            && ti < text.len()
            && pattern[pi + 1] == text[ti]
            && glob_match_rec(pattern, pi + 2, text, ti + 1, sep, captures);
    }
    if pattern[pi] == '*' {
        let double = pi + 1 < pattern.len() && pattern[pi + 1] == '*';
        let next_pi = if double { pi + 2 } else { pi + 1 };
        let mut end = ti;
        while end <= text.len() {
            if !double && text[ti..end].contains(&sep) {
                break;
            }
            let captured: String = text[ti..end].iter().collect();
            captures.insert_index(captured);
            if glob_match_rec(pattern, next_pi, text, end, sep, captures) {
                return true;
            }
            captures.indexed.pop();
            end += 1;
        }
        return false;
    }
    ti < text.len()
        && pattern[pi] == text[ti]
        && glob_match_rec(pattern, pi + 1, text, ti + 1, sep, captures)
}

pub(super) fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

pub(super) fn chance(req: &RequestMeta, line: usize, permille: u16) -> bool {
    if permille >= 1000 {
        return true;
    }
    if permille == 0 {
        return false;
    }
    let mut hash = 1469598103934665603u64;
    for byte in req.url.as_bytes().iter().chain(req.method.as_bytes()) {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash ^= line as u64;
    (hash % 1000) < permille as u64
}
