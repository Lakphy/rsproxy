use super::*;

pub(in crate::proxy) fn apply_header_op(
    headers: &mut Vec<(String, String)>,
    op: &HeaderOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    let limit = rule_header_limit(state);
    match op {
        HeaderOp::Set { name, value } => {
            http::set_header(
                headers,
                name,
                resolve_value_text_bounded(value, item, meta, state, limit)?,
            );
        }
        HeaderOp::Remove { name } => http::remove_header(headers, name),
        HeaderOp::Replace {
            name,
            pattern,
            replacement,
        } => {
            for (_, value) in headers
                .iter_mut()
                .filter(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
            {
                *value = pattern
                    .replace_all_bounded(value, replacement, limit)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            }
        }
    }
    Ok(())
}

pub(in crate::proxy) fn validate_header_block(
    headers: &[(String, String)],
    state: &SharedState,
) -> io::Result<()> {
    if headers.len() > state.config.max_header_count {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "rule actions produced {} headers, exceeding the configured limit of {}",
                headers.len(),
                state.config.max_header_count
            ),
        ));
    }
    let limit = rule_header_limit(state);
    let mut bytes = 0usize;
    for (name, value) in headers {
        if name.is_empty() || !name.bytes().all(is_http_token_byte) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("rule actions produced invalid HTTP header name `{name}`"),
            ));
        }
        if !value.bytes().all(is_http_field_value_byte) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("rule actions produced an invalid value for HTTP header `{name}`"),
            ));
        }
        bytes = bytes
            .checked_add(name.len())
            .and_then(|bytes| bytes.checked_add(value.len()))
            .and_then(|bytes| bytes.checked_add(4))
            .ok_or_else(|| output_limit_error("rule-produced header block", limit))?;
        if bytes > limit {
            return Err(output_limit_error("rule-produced header block", limit));
        }
    }
    Ok(())
}

pub(in crate::proxy) fn validate_http_method(method: &str) -> io::Result<()> {
    if method.is_empty() || !method.bytes().all(is_http_token_byte) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "rule actions produced an invalid HTTP method",
        ));
    }
    Ok(())
}

use rsproxy_rules::is_http_token_byte;

fn is_http_field_value_byte(byte: u8) -> bool {
    byte == b'\t' || (byte >= b' ' && byte != 0x7f)
}
