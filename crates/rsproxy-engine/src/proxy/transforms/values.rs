use super::*;

#[cfg(test)]
pub(in crate::proxy) fn resolve_value_text(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<String> {
    resolve_value_text_bounded(
        value,
        item,
        meta,
        state,
        rsproxy_rules::MAX_RULE_RENDERED_VALUE_BYTES,
    )
}

pub(in crate::proxy) fn resolve_value_text_bounded(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
    limit: usize,
) -> io::Result<String> {
    String::from_utf8(resolve_value_bytes_bounded(
        value, item, meta, state, limit,
    )?)
    .map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("text action value must be valid UTF-8: {error}"),
        )
    })
}

pub(in crate::proxy) fn resolve_raw_value_text(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<String> {
    let bytes = match value {
        Value::Inline(text) => return Ok(text.clone()),
        Value::File(path) => {
            let path = render_rule_path(path, item, meta)?;
            read_file_bytes(&path, state, rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES)?
        }
        Value::Reference(key) => read_reference_bytes(key, state)?,
    };
    String::from_utf8(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("regex replacement value must be valid UTF-8: {error}"),
        )
    })
}

#[cfg(test)]
pub(in crate::proxy) fn resolve_value_bytes(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<Vec<u8>> {
    resolve_value_bytes_bounded(
        value,
        item,
        meta,
        state,
        rsproxy_rules::MAX_RULE_RENDERED_VALUE_BYTES,
    )
}

pub(in crate::proxy) fn resolve_value_bytes_bounded(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let limit = limit.min(rsproxy_rules::MAX_RULE_RENDERED_VALUE_BYTES);
    match value {
        Value::Inline(text) => Ok(render_action_value(text, item, meta, limit)?.into_bytes()),
        Value::File(path) => {
            let path = render_rule_path(path, item, meta)?;
            let bytes = read_file_bytes(
                &path,
                state,
                limit.min(rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES),
            )?;
            render_text_bytes(bytes, item, meta, limit)
        }
        Value::Reference(key) => {
            let bytes = read_reference_bytes_bounded(key, state, limit)?;
            render_text_bytes(bytes, item, meta, limit)
        }
    }
}

pub(in crate::proxy) fn render_text_bytes(
    bytes: Vec<u8>,
    item: &ResolvedAction,
    meta: &RequestMeta,
    limit: usize,
) -> io::Result<Vec<u8>> {
    let limit = limit.min(rsproxy_rules::MAX_RULE_RENDERED_VALUE_BYTES);
    match String::from_utf8(bytes) {
        Ok(text) => Ok(render_action_value(&text, item, meta, limit)?.into_bytes()),
        Err(error) => Ok(error.into_bytes()),
    }
}

pub(in crate::proxy) fn render_action_value(
    input: &str,
    item: &ResolvedAction,
    meta: &RequestMeta,
    limit: usize,
) -> io::Result<String> {
    item.render_bounded(input, meta, limit).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("rule value rendering failed: {error}"),
        )
    })
}

pub(in crate::proxy) fn render_rule_path(
    path: &str,
    item: &ResolvedAction,
    meta: &RequestMeta,
) -> io::Result<String> {
    render_action_value(
        path,
        item,
        meta,
        rsproxy_rules::MAX_RULE_EXTERNAL_PATH_BYTES,
    )
}

pub(in crate::proxy) fn read_reference_bytes(
    key: &str,
    state: &SharedState,
) -> io::Result<Vec<u8>> {
    read_reference_bytes_bounded(key, state, rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES)
}

fn read_reference_bytes_bounded(
    key: &str,
    state: &SharedState,
    limit: usize,
) -> io::Result<Vec<u8>> {
    if !rsproxy_rules::valid_value_key(key) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid value key `{key}`"),
        ));
    }
    crate::bounded_io::read_file(
        &state.config.storage.join("values").join(key),
        limit.min(rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES),
        "rule value",
    )
}

fn read_file_bytes(path: &str, state: &SharedState, limit: usize) -> io::Result<Vec<u8>> {
    let storage_path = state.config.storage.join(path);
    read_external_value(&storage_path, limit).or_else(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            read_external_value(Path::new(path), limit)
        } else {
            Err(error)
        }
    })
}

fn read_external_value(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    crate::bounded_io::read_file(
        path,
        limit.min(rsproxy_rules::MAX_RULE_EXTERNAL_VALUE_BYTES),
        "rule value",
    )
}
