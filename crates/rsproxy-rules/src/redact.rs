pub fn redact_secrets(input: &str) -> String {
    let mut out = input.to_string();
    for scheme in ["socks5://", "socks://"] {
        let mut search_from = 0;
        while let Some(rel) = out[search_from..].find(scheme) {
            let start = search_from + rel + scheme.len();
            let Some(at_rel) = out[start..].find('@') else {
                break;
            };
            let at = start + at_rel;
            let credentials = &out[start..at];
            let authority_end = out[start..]
                .find(|ch: char| ch.is_whitespace() || matches!(ch, ')' | ',' | ']'))
                .map(|idx| start + idx)
                .unwrap_or(out.len());
            if at < authority_end && credentials.contains(':') && !credentials.contains('/') {
                out.replace_range(start..at, "auth");
                search_from = start + "auth@".len();
            } else {
                search_from = at + 1;
            }
        }
    }
    out
}
