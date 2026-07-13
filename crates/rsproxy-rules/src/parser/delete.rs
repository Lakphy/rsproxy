use super::*;

pub(super) fn parse_delete_ops(args: &[&str]) -> Result<Vec<DeleteOp>, RuleModelError> {
    if args.is_empty() {
        return Err(RuleModelError::missing(
            "delete action",
            "delete requires at least one property",
        ));
    }

    let mut operations = Vec::new();
    for raw in args {
        let property = unquote(raw.trim());
        if property.is_empty() {
            return Err(RuleModelError::empty(
                "delete property",
                "delete properties must not be empty",
            ));
        }
        parse_delete_property(&property, &mut operations)?;
    }
    Ok(operations)
}

fn parse_delete_property(
    property: &str,
    operations: &mut Vec<DeleteOp>,
) -> Result<(), RuleModelError> {
    if property.eq_ignore_ascii_case("pathname") {
        operations.push(DeleteOp::Pathname);
        return Ok(());
    }
    if let Some(segment) = suffix(property, "pathname.") {
        let segment = match segment.to_ascii_lowercase().as_str() {
            "first" => DeletePathSegment::Index(0),
            "last" => DeletePathSegment::Last,
            _ => DeletePathSegment::Index(segment.parse::<i32>().map_err(|source| {
                RuleModelError::integer(
                    "pathname segment",
                    segment,
                    format!("invalid pathname segment `{segment}`"),
                    source,
                )
            })?),
        };
        operations.push(DeleteOp::PathSegment(segment));
        return Ok(());
    }

    if matches_name(property, &["urlParams", "url.params", "params", "query"]) {
        operations.push(DeleteOp::UrlParams);
        return Ok(());
    }
    if let Some(name) = suffix_any(
        property,
        &["urlParams.", "url.params.", "params.", "query."],
    ) {
        operations.push(DeleteOp::UrlParam(valid_name("URL parameter", name)?));
        return Ok(());
    }

    if let Some(name) = suffix_any(
        property,
        &[
            "reqHeaders.",
            "reqHeader.",
            "req.headers.",
            "req.header.",
            "reqH.",
        ],
    ) {
        operations.push(DeleteOp::ReqHeader(valid_name("request header", name)?));
        return Ok(());
    }
    if let Some(name) = suffix_any(
        property,
        &[
            "resHeaders.",
            "resHeader.",
            "res.headers.",
            "res.header.",
            "resH.",
        ],
    ) {
        operations.push(DeleteOp::ResHeader(valid_name("response header", name)?));
        return Ok(());
    }
    if let Some(name) = suffix(property, "headers.") {
        let name = valid_name("header", name)?;
        operations.push(DeleteOp::ReqHeader(name.clone()));
        operations.push(DeleteOp::ResHeader(name));
        return Ok(());
    }

    if matches_name(property, &["reqBody", "req.body"]) {
        operations.push(DeleteOp::ReqBody);
        return Ok(());
    }
    if matches_name(property, &["resBody", "res.body"]) {
        operations.push(DeleteOp::ResBody);
        return Ok(());
    }
    if property.eq_ignore_ascii_case("body") {
        operations.push(DeleteOp::ReqBody);
        operations.push(DeleteOp::ResBody);
        return Ok(());
    }
    if let Some(path) = suffix_any(property, &["reqBody.", "req.body."]) {
        operations.push(DeleteOp::ReqBodyPath(parse_body_path(path)?));
        return Ok(());
    }
    if let Some(path) = suffix_any(property, &["resBody.", "res.body."]) {
        operations.push(DeleteOp::ResBodyPath(parse_body_path(path)?));
        return Ok(());
    }

    if matches_name(property, &["reqType", "req.type"]) {
        operations.push(DeleteOp::ReqType);
        return Ok(());
    }
    if matches_name(property, &["resType", "res.type"]) {
        operations.push(DeleteOp::ResType);
        return Ok(());
    }
    if matches_name(property, &["reqCharset", "req.charset"]) {
        operations.push(DeleteOp::ReqCharset);
        return Ok(());
    }
    if matches_name(property, &["resCharset", "res.charset"]) {
        operations.push(DeleteOp::ResCharset);
        return Ok(());
    }

    if matches_name(property, &["reqCookies", "req.cookies"]) {
        operations.push(DeleteOp::ReqCookies);
        return Ok(());
    }
    if matches_name(property, &["resCookies", "res.cookies"]) {
        operations.push(DeleteOp::ResCookies);
        return Ok(());
    }
    if matches_name(property, &["cookies", "cookie"]) {
        operations.push(DeleteOp::ReqCookies);
        operations.push(DeleteOp::ResCookies);
        return Ok(());
    }
    if let Some(name) = suffix_any(
        property,
        &[
            "reqCookies.",
            "reqCookie.",
            "req.cookies.",
            "req.cookie.",
            "reqC.",
        ],
    ) {
        operations.push(DeleteOp::ReqCookie(valid_name("request cookie", name)?));
        return Ok(());
    }
    if let Some(name) = suffix_any(
        property,
        &[
            "resCookies.",
            "resCookie.",
            "res.cookies.",
            "res.cookie.",
            "resC.",
        ],
    ) {
        operations.push(DeleteOp::ResCookie(valid_name("response cookie", name)?));
        return Ok(());
    }
    if let Some(name) = suffix_any(property, &["cookies.", "cookie."]) {
        let name = valid_name("cookie", name)?;
        operations.push(DeleteOp::ReqCookie(name.clone()));
        operations.push(DeleteOp::ResCookie(name));
        return Ok(());
    }
    if let Some(name) = suffix(property, "trailer.") {
        operations.push(DeleteOp::Trailer(valid_name("trailer", name)?));
        return Ok(());
    }
    if matches_name(property, &["trailers", "trailer"]) {
        operations.push(DeleteOp::Trailers);
        return Ok(());
    }

    Err(RuleModelError::unsupported(
        "delete property",
        format!("unknown delete property `{property}`"),
    ))
}

fn matches_name(input: &str, names: &[&str]) -> bool {
    names.iter().any(|name| input.eq_ignore_ascii_case(name))
}

fn suffix_any<'a>(input: &'a str, prefixes: &[&str]) -> Option<&'a str> {
    prefixes.iter().find_map(|prefix| suffix(input, prefix))
}

fn suffix<'a>(input: &'a str, prefix: &str) -> Option<&'a str> {
    let (head, tail) = input.split_at_checked(prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then_some(tail)
}

fn valid_name(kind: &str, value: &str) -> Result<String, RuleModelError> {
    let value = value.trim();
    if value.is_empty() || value.chars().any(|ch| ch.is_control()) {
        Err(RuleModelError::invalid(
            "delete property name",
            format!("{kind} name must be non-empty and contain no control characters"),
        ))
    } else {
        Ok(value.to_string())
    }
}

const MAX_BODY_PATH_BYTES: usize = 16 * 1024;
const MAX_BODY_PATH_SEGMENTS: usize = 128;

fn parse_body_path(input: &str) -> Result<DeleteBodyPath, RuleModelError> {
    if input.is_empty() {
        return Err(RuleModelError::empty(
            "delete body path",
            "delete body path must not be empty",
        ));
    }
    if input.len() > MAX_BODY_PATH_BYTES {
        return Err(RuleModelError::limit(
            "delete body path",
            format!("delete body path exceeds {MAX_BODY_PATH_BYTES} bytes"),
        ));
    }

    let components = split_body_path(input)?;
    let mut segments = Vec::new();
    for component in components {
        parse_body_path_component(&component, &mut segments)?;
        if segments.len() > MAX_BODY_PATH_SEGMENTS {
            return Err(RuleModelError::limit(
                "delete body path",
                format!("delete body path exceeds {MAX_BODY_PATH_SEGMENTS} segments"),
            ));
        }
    }
    DeleteBodyPath::new(segments)
}

fn split_body_path(input: &str) -> Result<Vec<String>, RuleModelError> {
    let mut components = Vec::new();
    let mut current = String::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let escaped = chars.next().ok_or_else(|| {
                    RuleModelError::syntax(
                        "delete body path",
                        "delete body path ends with an escape",
                    )
                })?;
                current.push('\\');
                current.push(escaped);
            }
            '.' => components.push(std::mem::take(&mut current)),
            _ => current.push(ch),
        }
    }
    components.push(current);
    Ok(components)
}

fn parse_body_path_component(
    input: &str,
    segments: &mut Vec<DeleteBodyPathSegment>,
) -> Result<(), RuleModelError> {
    let chars = decode_body_path_component(input)?;
    let mut end = chars.len();
    let mut indexes = Vec::new();

    while end > 0 && chars[end - 1] == (']', false) {
        let Some(open) = (0..end - 1)
            .rev()
            .find(|index| chars[*index] == ('[', false))
        else {
            break;
        };
        let Some(index) = body_array_index(&chars[open + 1..end - 1])? else {
            break;
        };
        indexes.push(index);
        end = open;
    }

    if end > 0 || indexes.is_empty() {
        let key = chars[..end].iter().map(|(ch, _)| *ch).collect();
        segments.push(DeleteBodyPathSegment::Key(key));
    }
    indexes.reverse();
    segments.extend(indexes.into_iter().map(DeleteBodyPathSegment::Index));
    Ok(())
}

fn decode_body_path_component(input: &str) -> Result<Vec<(char, bool)>, RuleModelError> {
    let mut decoded = Vec::new();
    let mut chars = input.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push((ch, false));
            continue;
        }
        let escaped = chars.next().ok_or_else(|| {
            RuleModelError::syntax("delete body path", "delete body path ends with an escape")
        })?;
        let escaped = match escaped {
            'n' => '\n',
            'r' => '\r',
            't' => '\t',
            'f' => '\u{000c}',
            'v' => '\u{000b}',
            other => other,
        };
        decoded.push((escaped, true));
    }
    Ok(decoded)
}

fn body_array_index(chars: &[(char, bool)]) -> Result<Option<usize>, RuleModelError> {
    if chars.is_empty()
        || chars
            .iter()
            .any(|(ch, escaped)| *escaped || !ch.is_ascii_digit())
        || (chars.len() > 1 && chars[0].0 == '0')
    {
        return Ok(None);
    }
    let value = chars.iter().map(|(ch, _)| *ch).collect::<String>();
    value.parse::<usize>().map(Some).map_err(|source| {
        RuleModelError::integer(
            "delete body array index",
            &value,
            format!("delete body array index is out of range: `{value}`"),
            source,
        )
    })
}
