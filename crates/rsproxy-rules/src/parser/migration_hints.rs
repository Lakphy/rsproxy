/// Recognizes common raw Whistle operator tokens and suggests the rsproxy form.
pub(super) fn whistle_syntax_hint(input: &str) -> Option<String> {
    if input == "$0" {
        return Some(
            "`$0` is Whistle syntax for \"leave this request alone\"; use `direct skip()` in rsproxy"
                .to_string(),
        );
    }
    for prefix in ["socks://", "socks5://"] {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some(format!(
                "`{input}` looks like a Whistle proxy target; use `upstream(socks5://{rest})`"
            ));
        }
    }
    for prefix in ["proxy://", "http-proxy://"] {
        if let Some(rest) = input.strip_prefix(prefix) {
            return Some(format!(
                "`{input}` looks like a Whistle proxy target; use `upstream(proxy://{rest})`"
            ));
        }
    }
    if input.starts_with("https-proxy://") {
        return Some(format!(
            "`{input}` looks like a Whistle proxy target; use `upstream({input})`"
        ));
    }
    if input.starts_with("http://") || input.starts_with("https://") {
        return Some(format!(
            "`{input}` looks like a Whistle map target; use `map.remote({input})` for a \
             transparent URL-preserving forward or `redirect({input})` for a client-visible 30x"
        ));
    }
    if !input.contains('(') && looks_like_host_port(input) {
        return Some(format!(
            "`{input}` looks like a Whistle host mapping; use `host({input})` to keep the \
             original Host header or `map.remote(http://{input})` for a transparent forward"
        ));
    }
    None
}

fn looks_like_host_port(input: &str) -> bool {
    let Some((host, port)) = input.rsplit_once(':') else {
        return false;
    };
    !host.is_empty()
        && port.chars().all(|ch| ch.is_ascii_digit())
        && !port.is_empty()
        && host
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
}
