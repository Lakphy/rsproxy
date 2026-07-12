use super::*;

pub(in crate::proxy) fn apply_header_op(
    headers: &mut Vec<(String, String)>,
    op: &HeaderOp,
    item: &ResolvedAction,
    meta: &RequestMeta,
    state: &SharedState,
) -> io::Result<()> {
    match op {
        HeaderOp::Set { name, value } => {
            http::set_header(headers, name, resolve_value_text(value, item, meta, state)?);
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
                *value = pattern.replace_all(value, replacement);
            }
        }
    }
    Ok(())
}
