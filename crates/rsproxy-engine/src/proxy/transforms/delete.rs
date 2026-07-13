use super::*;
use std::collections::BTreeSet;

mod body;

use body::{delete_request_body_path, delete_response_body_path};

pub(in crate::proxy) fn apply_url_delete(url: &mut UrlParts, operations: &[DeleteOp]) {
    if operations
        .iter()
        .any(|operation| matches!(operation, DeleteOp::Pathname))
    {
        url.path = "/".to_string();
    } else {
        let segments = operations
            .iter()
            .filter_map(|operation| match operation {
                DeleteOp::PathSegment(segment) => Some(*segment),
                _ => None,
            })
            .collect::<Vec<_>>();
        if !segments.is_empty() {
            delete_path_segments(&mut url.path, &segments);
        }
    }

    for operation in operations {
        match operation {
            DeleteOp::UrlParams => url.query = None,
            DeleteOp::UrlParam(name) => {
                let mut pairs = parse_query_pairs(url.query.as_deref().unwrap_or(""));
                pairs.retain(|(key, _)| key != name);
                url.query = (!pairs.is_empty()).then(|| {
                    pairs
                        .into_iter()
                        .map(|(key, value)| format!("{key}={value}"))
                        .collect::<Vec<_>>()
                        .join("&")
                });
            }
            _ => {}
        }
    }
}

pub(in crate::proxy) fn apply_request_delete(
    req: &mut RawRequest,
    operations: &[DeleteOp],
    body_available: bool,
) -> bool {
    let mut body_changed = false;
    for operation in operations {
        match operation {
            DeleteOp::ReqHeader(name) => http::remove_header(&mut req.headers, name),
            DeleteOp::ReqCookie(name) => remove_request_cookie(&mut req.headers, name),
            DeleteOp::ReqCookies => http::remove_header(&mut req.headers, "cookie"),
            DeleteOp::ReqType => remove_content_type_part(&mut req.headers, true),
            DeleteOp::ReqCharset => remove_content_type_part(&mut req.headers, false),
            DeleteOp::ReqBody if body_available => {
                req.body.clear();
                body_changed = true;
            }
            DeleteOp::ReqBodyPath(path) if body_available => {
                body_changed |= delete_request_body_path(&req.headers, &mut req.body, path);
            }
            _ => {}
        }
    }
    body_changed
}

pub(in crate::proxy) fn apply_response_delete(
    headers: &mut Vec<(String, String)>,
    body: &mut Vec<u8>,
    operations: &[DeleteOp],
    body_available: bool,
) {
    for operation in operations {
        match operation {
            DeleteOp::ResHeader(name) => http::remove_header(headers, name),
            DeleteOp::ResCookie(name) => remove_response_cookie(headers, name),
            DeleteOp::ResCookies => http::remove_header(headers, "set-cookie"),
            DeleteOp::ResType => remove_content_type_part(headers, true),
            DeleteOp::ResCharset => remove_content_type_part(headers, false),
            DeleteOp::ResBody if body_available => body.clear(),
            DeleteOp::ResBodyPath(path) if body_available => {
                delete_response_body_path(headers, body, path);
            }
            _ => {}
        }
    }
}

pub(in crate::proxy) fn apply_trailer_delete(
    trailers: &mut Vec<(String, String)>,
    operations: &[DeleteOp],
) {
    for operation in operations {
        if let DeleteOp::Trailer(name) = operation {
            http::remove_header(trailers, name);
        } else if matches!(operation, DeleteOp::Trailers) {
            trailers.clear();
        }
    }
}

fn delete_path_segments(path: &mut String, operations: &[DeletePathSegment]) {
    let raw = path.strip_prefix('/').unwrap_or(path);
    let mut segments = raw.split('/').map(str::to_string).collect::<Vec<_>>();
    let len = segments.len() as i64;
    let mut removed = BTreeSet::new();
    let mut preserve_trailing_slash = false;

    for operation in operations {
        let index = match operation {
            DeletePathSegment::Index(index) if *index < 0 => len + i64::from(*index),
            DeletePathSegment::Index(index) => i64::from(*index),
            DeletePathSegment::Last => {
                preserve_trailing_slash = true;
                len - 1
            }
        };
        if (0..len).contains(&index) {
            removed.insert(index as usize);
        }
    }

    segments = segments
        .into_iter()
        .enumerate()
        .filter_map(|(index, segment)| (!removed.contains(&index)).then_some(segment))
        .collect();
    if preserve_trailing_slash && segments.last().is_some_and(|segment| !segment.is_empty()) {
        segments.push(String::new());
    }
    let joined = segments.join("/");
    *path = if joined.is_empty() {
        "/".to_string()
    } else {
        format!("/{joined}")
    };
}

fn remove_request_cookie(headers: &mut Vec<(String, String)>, name: &str) {
    let mut cookies = http::header(headers, "cookie")
        .map(parse_cookie_header)
        .unwrap_or_default();
    cookies.retain(|(cookie_name, _)| cookie_name != name);
    if cookies.is_empty() {
        http::remove_header(headers, "cookie");
    } else {
        http::set_header(
            headers,
            "Cookie",
            cookies
                .into_iter()
                .map(|(cookie_name, value)| format!("{cookie_name}={value}"))
                .collect::<Vec<_>>()
                .join("; "),
        );
    }
}

fn remove_response_cookie(headers: &mut Vec<(String, String)>, name: &str) {
    headers.retain(|(header_name, value)| {
        !(header_name.eq_ignore_ascii_case("set-cookie")
            && value
                .split_once('=')
                .is_some_and(|(cookie_name, _)| cookie_name.trim() == name))
    });
}

fn remove_content_type_part(headers: &mut Vec<(String, String)>, remove_type: bool) {
    let Some(value) = http::header(headers, "content-type") else {
        return;
    };
    let mut parts = value
        .trim()
        .split(';')
        .map(str::trim)
        .map(str::to_string)
        .collect::<Vec<_>>();
    if remove_type {
        if let Some(mime) = parts.first_mut() {
            mime.clear();
        }
    } else {
        parts.truncate(1);
    }
    let value = parts.join("; ");
    if value.is_empty() {
        http::remove_header(headers, "content-type");
    } else {
        http::set_header(headers, "Content-Type", value);
    }
}

#[cfg(test)]
#[path = "delete/tests.rs"]
mod tests;
