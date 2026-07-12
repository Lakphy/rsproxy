use crate::http;
use rsproxy_rules::{DeleteBodyPath, DeleteBodyPathSegment};
use serde_json::Value as JsonValue;
use std::ops::Range;

pub(super) fn delete_request_body_path(
    headers: &[(String, String)],
    body: &mut Vec<u8>,
    path: &DeleteBodyPath,
) -> bool {
    if !plain_body(headers) {
        return false;
    }
    match media_type(headers).as_deref() {
        Some("application/x-www-form-urlencoded") => delete_form_field(body, path),
        Some(media_type) if json_media_type(media_type) => delete_json_field(body, path, false),
        _ => false,
    }
}

pub(super) fn delete_response_body_path(
    headers: &[(String, String)],
    body: &mut Vec<u8>,
    path: &DeleteBodyPath,
) -> bool {
    if !plain_body(headers) {
        return false;
    }
    match media_type(headers) {
        None => delete_json_field(body, path, false),
        Some(media_type) if json_media_type(&media_type) => delete_json_field(body, path, false),
        Some(media_type) if jsonp_media_type(&media_type) => delete_json_field(body, path, true),
        _ => false,
    }
}

fn plain_body(headers: &[(String, String)]) -> bool {
    http::header(headers, "content-encoding").is_none_or(|value| {
        value
            .split(',')
            .all(|encoding| encoding.trim().eq_ignore_ascii_case("identity"))
    })
}

fn media_type(headers: &[(String, String)]) -> Option<String> {
    http::header(headers, "content-type")
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_ascii_lowercase)
}

fn json_media_type(media_type: &str) -> bool {
    matches!(media_type, "application/json" | "text/json") || media_type.ends_with("+json")
}

fn jsonp_media_type(media_type: &str) -> bool {
    matches!(
        media_type,
        "application/javascript"
            | "text/javascript"
            | "application/x-javascript"
            | "application/ecmascript"
            | "text/ecmascript"
            | "application/jsonp"
    )
}

fn delete_json_field(body: &mut Vec<u8>, path: &DeleteBodyPath, jsonp: bool) -> bool {
    let Ok(text) = std::str::from_utf8(body) else {
        return false;
    };
    let search_start = if jsonp {
        text.find('(').map(|index| index + 1).unwrap_or(0)
    } else {
        0
    };
    let Some((range, mut value)) = first_json_value(text, search_start) else {
        return false;
    };
    if !delete_json_path(&mut value, path.segments()) {
        return false;
    }
    let Ok(replacement) = serde_json::to_vec(&value) else {
        return false;
    };
    let mut output = Vec::with_capacity(
        body.len()
            .saturating_sub(range.len())
            .saturating_add(replacement.len()),
    );
    output.extend_from_slice(&body[..range.start]);
    output.extend_from_slice(&replacement);
    output.extend_from_slice(&body[range.end..]);
    *body = output;
    true
}

fn first_json_value(text: &str, search_start: usize) -> Option<(Range<usize>, JsonValue)> {
    let (offset, _) = text[search_start..]
        .char_indices()
        .find(|(_, ch)| matches!(ch, '{' | '['))?;
    let start = search_start + offset;
    let mut values = serde_json::Deserializer::from_str(&text[start..]).into_iter::<JsonValue>();
    let value = values.next()?.ok()?;
    if !matches!(&value, JsonValue::Object(_) | JsonValue::Array(_)) {
        return None;
    }
    Some((start..start + values.byte_offset(), value))
}

fn delete_json_path(value: &mut JsonValue, segments: &[DeleteBodyPathSegment]) -> bool {
    let Some((last, parents)) = segments.split_last() else {
        return false;
    };
    let mut parent = value;
    for segment in parents {
        let next = match parent {
            JsonValue::Object(object) => object.get_mut(&segment_key(segment)),
            JsonValue::Array(array) => {
                segment_index(segment).and_then(|index| array.get_mut(index))
            }
            _ => None,
        };
        let Some(next) = next else {
            return false;
        };
        parent = next;
    }
    match parent {
        JsonValue::Object(object) => object.remove(&segment_key(last)).is_some(),
        JsonValue::Array(array) => segment_index(last)
            .filter(|index| *index < array.len())
            .map(|index| {
                array.remove(index);
                true
            })
            .unwrap_or(false),
        _ => false,
    }
}

fn segment_key(segment: &DeleteBodyPathSegment) -> String {
    match segment {
        DeleteBodyPathSegment::Key(key) => key.clone(),
        DeleteBodyPathSegment::Index(index) => index.to_string(),
    }
}

fn segment_index(segment: &DeleteBodyPathSegment) -> Option<usize> {
    match segment {
        DeleteBodyPathSegment::Index(index) => Some(*index),
        DeleteBodyPathSegment::Key(key)
            if key == "0"
                || (!key.starts_with('0') && key.chars().all(|ch| ch.is_ascii_digit())) =>
        {
            key.parse().ok()
        }
        DeleteBodyPathSegment::Key(_) => None,
    }
}

fn delete_form_field(body: &mut Vec<u8>, path: &DeleteBodyPath) -> bool {
    let name = form_field_name(path);
    let fields = body.split(|byte| *byte == b'&').collect::<Vec<_>>();
    let kept = fields
        .iter()
        .copied()
        .filter(|field| field.split(|byte| *byte == b'=').next() != Some(name.as_bytes()))
        .collect::<Vec<_>>();
    if kept.len() == fields.len() {
        return false;
    }
    let mut output = Vec::new();
    for (index, field) in kept.into_iter().enumerate() {
        if index > 0 {
            output.push(b'&');
        }
        output.extend_from_slice(field);
    }
    *body = output;
    true
}

fn form_field_name(path: &DeleteBodyPath) -> String {
    let mut output = String::new();
    for (index, segment) in path.segments().iter().enumerate() {
        match segment {
            DeleteBodyPathSegment::Key(key) => {
                if index > 0 {
                    output.push('.');
                }
                output.push_str(key);
            }
            DeleteBodyPathSegment::Index(value) => output.push_str(&format!("[{value}]")),
        }
    }
    output
}

#[cfg(test)]
#[path = "body/tests.rs"]
mod tests;
