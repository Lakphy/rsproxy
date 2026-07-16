use super::*;

/// Parses `mock(value)` plus the inline `mock(status=..., header=..., body=...)` form.
pub(super) fn parse_mock(args: &[&str]) -> Result<Action, RuleModelError> {
    let inline_form = args.len() > 1
        || args.first().is_some_and(|arg| {
            let arg = arg.trim();
            ["status=", "body=", "type=", "header="]
                .iter()
                .any(|prefix| arg.starts_with(prefix))
        });
    if !inline_form {
        return Ok(Action::Mock(parse_value(require_one(args, "mock")?)?));
    }

    let mut op = MockInlineOp {
        status: None,
        headers: Vec::new(),
        body: None,
    };
    for arg in args {
        let arg = arg.trim();
        let Some((key, value)) = arg.split_once('=') else {
            return Err(RuleModelError::invalid(
                "mock argument",
                format!(
                    "inline mock arguments use key=value form (status, type, header, body); got `{arg}`"
                ),
            ));
        };
        match key.trim() {
            "status" => {
                let value = value.trim();
                let code = value.parse::<u16>().map_err(|source| {
                    RuleModelError::integer(
                        "mock status",
                        value,
                        "mock status must be numeric",
                        source,
                    )
                })?;
                if !(100..=999).contains(&code) {
                    return Err(RuleModelError::constraint(
                        "mock status",
                        format!("invalid mock status `{code}`"),
                    ));
                }
                op.status = Some(code);
            }
            "type" => {
                op.headers
                    .push(("Content-Type".to_string(), parse_value(value)?));
            }
            "header" => {
                let Some((name, value)) = value.split_once(':') else {
                    return Err(RuleModelError::invalid(
                        "mock header",
                        format!("mock header must use `header=Name: value`; got `{value}`"),
                    ));
                };
                let name = name.trim();
                if name.is_empty() {
                    return Err(RuleModelError::empty(
                        "mock header name",
                        "mock header name is empty",
                    ));
                }
                op.headers
                    .push((name.to_string(), parse_value(value.trim_start())?));
            }
            "body" => op.body = Some(parse_value(value)?),
            other => {
                return Err(RuleModelError::unsupported(
                    "mock argument",
                    format!(
                        "unknown inline mock argument `{other}`; use status, type, header, or body"
                    ),
                ));
            }
        }
    }
    Ok(Action::MockInline(op))
}

pub(super) fn validate_map_remote_target(value: &Value) -> Result<(), RuleModelError> {
    // Only literal inline targets are validated at parse time; templated,
    // file-backed, and referenced targets resolve when the request runs.
    let Value::Inline(target) = value else {
        return Ok(());
    };
    if target.contains('$') {
        return Ok(());
    }
    let scheme_ok = target.starts_with("http://") || target.starts_with("https://");
    if !scheme_ok {
        return Err(RuleModelError::invalid(
            "map.remote target",
            format!("map.remote target must start with http:// or https://; got `{target}`"),
        ));
    }
    Ok(())
}
