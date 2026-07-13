use rsproxy_rules::UrlParts;

pub(super) fn literal_ip_from_url(url: &str) -> Option<String> {
    let host = UrlParts::parse(url).ok()?.host;
    host.parse::<std::net::IpAddr>()
        .ok()
        .map(|ip| ip.to_string())
}

pub(super) fn split_query(target: &str) -> (&str, Option<&str>) {
    target
        .split_once('?')
        .map(|(path, query)| (path, Some(query)))
        .unwrap_or((target, None))
}

pub(super) fn query_get(query: Option<&str>, key: &str) -> Option<String> {
    let query = query?;
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if percent_decode(name) == key {
            return Some(percent_decode(value));
        }
    }
    None
}

pub(super) fn query_get_all(query: Option<&str>, key: &str) -> Vec<String> {
    let Some(query) = query else {
        return Vec::new();
    };
    query
        .split('&')
        .filter_map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            if percent_decode(name) == key {
                Some(percent_decode(value))
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn parse_header_query_value(value: &str) -> Option<(String, String)> {
    let (name, value) = value.split_once(':')?;
    let name = name.trim();
    if !valid_header_name(name) {
        None
    } else {
        Some((name.to_string(), value.trim_start().to_string()))
    }
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

pub(super) fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Ok(hex) = u8::from_str_radix(&input[index + 1..index + 3], 16)
        {
            output.push(hex);
            index += 3;
            continue;
        }
        if bytes[index] == b'+' {
            output.push(b' ');
        } else {
            output.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&output).to_string()
}
