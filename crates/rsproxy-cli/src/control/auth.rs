use crate::http;
use std::io::Write;

pub(super) fn control_authorized(headers: &[(String, String)], expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    headers.iter().any(|(name, value)| {
        let candidate = if name.eq_ignore_ascii_case("authorization") {
            bearer_token(value)
        } else if name.eq_ignore_ascii_case("x-rsproxy-token") {
            Some(value.trim())
        } else {
            None
        };
        candidate.is_some_and(|candidate| constant_time_eq(candidate, expected))
    })
}

pub(super) fn respond_control_unauthorized<W: Write + ?Sized>(
    stream: &mut W,
) -> std::io::Result<()> {
    http::write_response(
        stream,
        401,
        "Unauthorized",
        &[
            ("Content-Type".to_string(), "application/json".to_string()),
            ("WWW-Authenticate".to_string(), "Bearer".to_string()),
        ],
        b"{\"error\":\"unauthorized\"}",
    )
}

fn bearer_token(value: &str) -> Option<&str> {
    let mut parts = value.split_ascii_whitespace();
    let scheme = parts.next()?;
    let token = parts.next()?;
    if !scheme.eq_ignore_ascii_case("bearer") || parts.next().is_some() {
        return None;
    }
    Some(token)
}

fn constant_time_eq(candidate: &str, expected: &str) -> bool {
    let candidate = candidate.as_bytes();
    let expected = expected.as_bytes();
    let mut difference = candidate.len() ^ expected.len();
    for index in 0..candidate.len().max(expected.len()) {
        let left = candidate.get(index).copied().unwrap_or(0);
        let right = expected.get(index).copied().unwrap_or(0);
        difference |= usize::from(left ^ right);
    }
    difference == 0
}
