use super::*;
use crate::RuleModelError;
use crate::template::transform::validate_template;

impl Action {
    pub(crate) fn validate_templates(&self) -> Result<(), RuleModelError> {
        match self {
            Self::Host(pool) => validate_values(pool.addresses()),
            Self::Upstream(value)
            | Self::ReqMethod(value)
            | Self::ReqUa(value)
            | Self::ReqReferer(value)
            | Self::ReqAuth(value)
            | Self::ReqForwarded(value)
            | Self::ReqType(value)
            | Self::ReqCharset(value)
            | Self::ResType(value)
            | Self::ResCharset(value)
            | Self::ResMerge(value)
            | Self::Tag(value) => validate_value(value),
            Self::Mock(value) | Self::MockRaw(value) => validate_value(value),
            Self::Redirect { url, .. } => validate_value(url),
            Self::ReqHeader(operation)
            | Self::ResHeader(operation)
            | Self::ResTrailer(operation) => validate_header(operation),
            Self::ReqCookie(operation) | Self::ResCookie(operation) => validate_cookie(operation),
            Self::ResCors(operation) => validate_cors(operation),
            Self::Attachment(filename) => filename.as_ref().map(validate_value).unwrap_or(Ok(())),
            Self::Cache(CacheOp::Directives(directives)) => validate_values(
                directives
                    .iter()
                    .filter_map(|directive| directive.value.as_ref()),
            ),
            Self::Tls(operation) => validate_many(
                operation
                    .client_cert
                    .iter()
                    .chain(operation.client_key.iter())
                    .map(String::as_str),
            ),
            Self::UrlRewrite { from, to } => match from {
                UrlRewritePattern::Plain(from) => validate_values([from, to]),
                UrlRewritePattern::Regex(_) => validate_regex_replacement_value(to),
            },
            Self::UrlQuery(operations) => {
                validate_values(operations.iter().filter_map(|operation| match operation {
                    QueryOp::Set { value, .. } => Some(value),
                    QueryOp::Remove { .. } => None,
                }))
            }
            Self::ReqBody(operation) | Self::ResBody(operation) => validate_body(operation),
            Self::Inject(operation) => validate_value(&operation.value),
            Self::Direct
            | Self::Status(_)
            | Self::ResStatus(_)
            | Self::Cache(CacheOp::Off)
            | Self::Delay { .. }
            | Self::Throttle { .. }
            | Self::Bypass
            | Self::Hide
            | Self::Delete(_)
            | Self::Skip(_) => Ok(()),
        }
    }
}

fn validate_header(operation: &HeaderOp) -> Result<(), RuleModelError> {
    match operation {
        HeaderOp::Set { value, .. } => validate_value(value),
        HeaderOp::Remove { .. } | HeaderOp::Replace { .. } => Ok(()),
    }
}

fn validate_cookie(operation: &CookieOp) -> Result<(), RuleModelError> {
    match operation {
        CookieOp::Set { value, attrs, .. } => {
            validate_value(value)?;
            validate_values(
                attrs
                    .iter()
                    .filter_map(|attribute| attribute.value.as_ref()),
            )
        }
        CookieOp::Remove { .. } => Ok(()),
    }
}

fn validate_cors(operation: &CorsOp) -> Result<(), RuleModelError> {
    validate_values(
        [
            Some(&operation.origin),
            operation.methods.as_ref(),
            operation.headers.as_ref(),
            operation.expose.as_ref(),
            operation.max_age.as_ref(),
        ]
        .into_iter()
        .flatten(),
    )
}

fn validate_body(operation: &BodyOp) -> Result<(), RuleModelError> {
    match operation {
        BodyOp::Set(value) | BodyOp::Prepend(value) | BodyOp::Append(value) => {
            validate_value(value)
        }
        BodyOp::Replace { .. } => Ok(()),
    }
}

fn validate_value(value: &Value) -> Result<(), RuleModelError> {
    match value {
        Value::Inline(value) | Value::File(value) => validate_template(value),
        Value::Reference(_) => Ok(()),
    }
}

fn validate_many<'a>(values: impl IntoIterator<Item = &'a str>) -> Result<(), RuleModelError> {
    for value in values {
        validate_template(value)?;
    }
    Ok(())
}

fn validate_values<'a>(values: impl IntoIterator<Item = &'a Value>) -> Result<(), RuleModelError> {
    for value in values {
        validate_value(value)?;
    }
    Ok(())
}

fn validate_regex_replacement_value(value: &Value) -> Result<(), RuleModelError> {
    match value {
        Value::Inline(_) | Value::Reference(_) => Ok(()),
        Value::File(path) => validate_template(path),
    }
}
