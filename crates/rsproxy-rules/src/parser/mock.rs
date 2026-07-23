use super::*;
use crate::language::status_forbids_body;

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
                validate_status_range(
                    code,
                    MIN_FINAL_HTTP_STATUS..=MAX_HTTP_STATUS,
                    "mock status",
                    "200..599",
                )?;
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
                let name = normalize_header_name(name)?;
                op.headers.push((name, parse_value(value.trim_start())?));
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
    if op.status.is_some_and(status_forbids_body) && op.body.is_some() {
        return Err(RuleModelError::constraint(
            "inline mock body",
            "mock status 204, 205, or 304 must not define a body",
        ));
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
    let parsed = UrlParts::parse(target)?;
    if !matches!(parsed.scheme.as_str(), "http" | "https") {
        return Err(RuleModelError::invalid(
            "map.remote target",
            format!("map.remote target must use http or https; got `{target}`"),
        ));
    }
    Ok(())
}
