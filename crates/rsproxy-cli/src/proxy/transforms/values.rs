use super::*;

pub(in crate::proxy) fn resolve_value_text(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<String> {
    String::from_utf8(resolve_value_bytes(value, item, meta, state)?).map_err(|error| {
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
        Value::File(path) => read_file_bytes(&item.render(path, meta), state)?,
        Value::Reference(key) => read_reference_bytes(key, state)?,
    };
    String::from_utf8(bytes).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("regex replacement value must be valid UTF-8: {error}"),
        )
    })
}

pub(in crate::proxy) fn resolve_value_bytes(
    value: &Value,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<Vec<u8>> {
    match value {
        Value::Inline(text) => Ok(item.render(text, meta).into_bytes()),
        Value::File(path) => {
            let bytes = read_file_bytes(&item.render(path, meta), state)?;
            Ok(render_text_bytes(bytes, item, meta))
        }
        Value::Reference(key) => {
            let bytes = read_reference_bytes(key, state)?;
            Ok(render_text_bytes(bytes, item, meta))
        }
    }
}

pub(in crate::proxy) fn render_text_bytes(
    bytes: Vec<u8>,
    item: &ResolvedAction,
    meta: &RequestMeta,
) -> Vec<u8> {
    match String::from_utf8(bytes) {
        Ok(text) => item.render(&text, meta).into_bytes(),
        Err(error) => error.into_bytes(),
    }
}

pub(in crate::proxy) fn read_reference_bytes(
    key: &str,
    state: &SharedState,
) -> io::Result<Vec<u8>> {
    if !rsproxy_rules::valid_value_key(key) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid value key `{key}`"),
        ));
    }
    fs::read(state.config.storage.join("values").join(key))
}

fn read_file_bytes(path: &str, state: &SharedState) -> io::Result<Vec<u8>> {
    let storage_path = state.config.storage.join(path);
    fs::read(&storage_path).or_else(|_| fs::read(path))
}
