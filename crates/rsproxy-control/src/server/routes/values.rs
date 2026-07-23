use super::super::http;
use super::super::query::percent_decode;
use super::super::values::{valid_value_key, value_keys};
use super::ControlState;
use super::respond_json;
use crate::shapes;
use std::fs;
use std::io::{Read, Write};
use std::path::Path;

const PREFIX: &str = "/api/values/";

pub(super) fn list<W: Write + ?Sized>(stream: &mut W, state: &ControlState) -> std::io::Result<()> {
    let keys = value_keys(state);
    respond_json(
        stream,
        200,
        &format!(
            "[{}]",
            keys.iter()
                .map(|key| shapes::string(key))
                .collect::<Vec<_>>()
                .join(",")
        ),
    )
}

pub(super) fn list_text<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
) -> std::io::Result<()> {
    let body = value_keys(state).join("\n");
    http::write_response(
        stream,
        200,
        "OK",
        &[("Content-Type".to_string(), "text/plain".to_string())],
        body.as_bytes(),
    )
}

pub(super) fn get<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    path: &str,
) -> std::io::Result<()> {
    let Some(key) = key_from_path(path) else {
        return respond_json(stream, 400, "{\"error\":\"invalid key\"}");
    };
    match read_value(&state.options.storage.join("values").join(&key)) {
        Ok(body) => http::write_response(
            stream,
            200,
            "OK",
            &[(
                "Content-Type".to_string(),
                "application/octet-stream".to_string(),
            )],
            &body,
        ),
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            respond_json(stream, 413, "{\"error\":\"value exceeds size limit\"}")
        }
        Err(_) => respond_json(stream, 404, "{\"error\":\"not found\"}"),
    }
}

pub(super) fn set<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    path: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let Some(key) = key_from_path(path) else {
        return respond_json(stream, 400, "{\"error\":\"invalid key\"}");
    };
    if body.len() > rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES {
        return respond_json(stream, 413, "{\"error\":\"value exceeds size limit\"}");
    }
    let values_dir = state.options.storage.join("values");
    fs::create_dir_all(&values_dir)?;
    fs::write(values_dir.join(key), body)?;
    respond_json(stream, 200, "{\"ok\":true}")
}

pub(super) fn delete<W: Write + ?Sized>(
    stream: &mut W,
    state: &ControlState,
    path: &str,
) -> std::io::Result<()> {
    let Some(key) = key_from_path(path) else {
        return respond_json(stream, 400, "{\"error\":\"invalid key\"}");
    };
    let _ = fs::remove_file(state.options.storage.join("values").join(key));
    respond_json(stream, 200, "{\"ok\":true}")
}

fn key_from_path(path: &str) -> Option<String> {
    let key = percent_decode(path.strip_prefix(PREFIX)?);
    valid_value_key(&key).then_some(key)
}

fn read_value(path: &Path) -> std::io::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let limit = rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES;
    let mut body = Vec::with_capacity(limit.min(64 * 1024));
    file.take(limit.saturating_add(1) as u64)
        .read_to_end(&mut body)?;
    if body.len() > limit {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("rule value exceeds the {limit}-byte limit"),
        ));
    }
    Ok(body)
}
