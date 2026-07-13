use super::*;

pub(super) fn authorized(req: &RawRequest, expected: Option<&str>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Some(actual) = http::header(&req.headers, "proxy-authorization") else {
        return false;
    };
    let mut parts = actual.split_whitespace();
    let Some(scheme) = parts.next() else {
        return false;
    };
    let Some(credentials) = parts.next() else {
        return false;
    };
    if parts.next().is_some() || !scheme.eq_ignore_ascii_case("basic") {
        return false;
    }
    credentials == base64(expected.as_bytes())
}

pub(super) fn authorize_and_strip_proxy_credentials(
    req: &mut RawRequest,
    expected: Option<&str>,
) -> bool {
    if !authorized(req, expected) {
        return false;
    }
    http::remove_header(&mut req.headers, "proxy-authorization");
    true
}

pub(super) fn base64(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < input.len() {
        let b0 = input[i];
        let b1 = *input.get(i + 1).unwrap_or(&0);
        let b2 = *input.get(i + 2).unwrap_or(&0);
        out.push(TABLE[(b0 >> 2) as usize] as char);
        out.push(TABLE[(((b0 & 0b11) << 4) | (b1 >> 4)) as usize] as char);
        if i + 1 < input.len() {
            out.push(TABLE[(((b1 & 0b1111) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if i + 2 < input.len() {
            out.push(TABLE[(b2 & 0b111111) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}
