//! Shared, side-effect-free HTTP protocol semantics.
//!
//! This crate deliberately contains no transport or rule-language behavior. It
//! centralizes policy that must remain identical in parsers, serializers, and
//! proxy execution paths.

/// Reports whether an upstream response is framed as carrying body bytes.
///
/// HTTP/1 framing excludes responses to `HEAD`, successful `CONNECT`
/// responses, informational responses, 204, and 304. Status 205 is
/// intentionally not excluded here: a non-conforming peer can frame bytes that
/// a proxy must consume to keep the connection synchronized.
pub fn response_has_framed_body(method: &str, status: u16) -> bool {
    !method.eq_ignore_ascii_case("HEAD")
        && !(method.eq_ignore_ascii_case("CONNECT") && (200..300).contains(&status))
        && !matches!(status, 100..=199 | 204 | 304)
}

/// Reports whether a response status permits a sender to generate content.
pub const fn status_can_send_content(status: u16) -> bool {
    !matches!(status, 100..=199 | 204 | 205 | 304)
}

/// Reports whether a response may send content for the request method and status.
pub fn response_can_send_content(method: &str, status: u16) -> bool {
    !method.eq_ignore_ascii_case("HEAD")
        && !(method.eq_ignore_ascii_case("CONNECT") && (200..300).contains(&status))
        && status_can_send_content(status)
}

/// Reports whether a field name is forbidden in an HTTP trailer section.
///
/// The list covers framing, connection, routing, authentication, request and
/// response control, and representation metadata that recipients need before
/// consuming content. Callers must additionally reject names nominated by a
/// message's `Connection` field.
pub fn is_forbidden_trailer_name(name: &str) -> bool {
    const FORBIDDEN: &[&str] = &[
        "age",
        "authorization",
        "cache-control",
        "connection",
        "content-encoding",
        "content-length",
        "content-range",
        "content-type",
        "date",
        "expect",
        "expires",
        "host",
        "keep-alive",
        "location",
        "max-forwards",
        "pragma",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "range",
        "retry-after",
        "te",
        "trailer",
        "transfer-encoding",
        "upgrade",
        "vary",
        "warning",
        "www-authenticate",
    ];
    FORBIDDEN
        .iter()
        .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
}

#[cfg(test)]
mod tests;
