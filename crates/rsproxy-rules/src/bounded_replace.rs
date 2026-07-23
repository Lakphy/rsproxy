use crate::RuleModelError;
use regex::{Captures, Regex};

pub(crate) fn regex_replace_all(
    regex: &Regex,
    input: &str,
    replacement: &str,
    limit: usize,
    context: &'static str,
) -> Result<String, RuleModelError> {
    let template = ReplacementTemplate::parse(replacement);
    let mut output = String::with_capacity(input.len().min(limit));
    let mut last = 0usize;
    for captures in regex.captures_iter(input) {
        let matched = captures
            .get(0)
            .expect("regex capture iteration always includes the complete match");
        push_bounded(&mut output, &input[last..matched.start()], limit, context)?;
        template.expand_bounded(&captures, &mut output, limit, context)?;
        last = matched.end();
    }
    push_bounded(&mut output, &input[last..], limit, context)?;
    Ok(output)
}

#[derive(Clone, Copy)]
enum ReplacementToken<'a> {
    Literal(&'a str),
    CaptureIndex(usize),
    CaptureName(&'a str),
    Dollar,
}

struct ReplacementTemplate<'a> {
    tokens: Vec<ReplacementToken<'a>>,
}

impl<'a> ReplacementTemplate<'a> {
    fn parse(replacement: &'a str) -> Self {
        let mut tokens = Vec::new();
        let mut remaining = replacement;
        while !remaining.is_empty() {
            let Some(offset) = remaining.as_bytes().iter().position(|byte| *byte == b'$') else {
                tokens.push(ReplacementToken::Literal(remaining));
                break;
            };
            if offset > 0 {
                tokens.push(ReplacementToken::Literal(&remaining[..offset]));
                remaining = &remaining[offset..];
            }
            if remaining.as_bytes().get(1) == Some(&b'$') {
                tokens.push(ReplacementToken::Dollar);
                remaining = &remaining[2..];
                continue;
            }
            let Some((token, end)) = capture_token(remaining) else {
                tokens.push(ReplacementToken::Literal("$"));
                remaining = &remaining[1..];
                continue;
            };
            tokens.push(token);
            remaining = &remaining[end..];
        }
        Self { tokens }
    }

    fn expand_bounded(
        &self,
        captures: &Captures<'_>,
        output: &mut String,
        limit: usize,
        context: &'static str,
    ) -> Result<(), RuleModelError> {
        for token in &self.tokens {
            let value = match token {
                ReplacementToken::Literal(value) => Some(*value),
                ReplacementToken::CaptureIndex(index) => {
                    captures.get(*index).map(|matched| matched.as_str())
                }
                ReplacementToken::CaptureName(name) => {
                    captures.name(name).map(|matched| matched.as_str())
                }
                ReplacementToken::Dollar => Some("$"),
            };
            if let Some(value) = value {
                push_bounded(output, value, limit, context)?;
            }
        }
        Ok(())
    }
}

fn capture_token(replacement: &str) -> Option<(ReplacementToken<'_>, usize)> {
    let bytes = replacement.as_bytes();
    if bytes.len() <= 1 || bytes[0] != b'$' {
        return None;
    }
    if bytes[1] == b'{' {
        let close = bytes[2..].iter().position(|byte| *byte == b'}')? + 2;
        let name = &replacement[2..close];
        return Some((capture_name_or_index(name), close + 1));
    }
    let end = bytes[1..]
        .iter()
        .position(|byte| !matches!(byte, b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'_'))
        .map_or(bytes.len(), |offset| offset + 1);
    if end == 1 {
        return None;
    }
    Some((capture_name_or_index(&replacement[1..end]), end))
}

fn capture_name_or_index(name: &str) -> ReplacementToken<'_> {
    name.parse::<usize>().map_or_else(
        |_| ReplacementToken::CaptureName(name),
        ReplacementToken::CaptureIndex,
    )
}

pub(crate) fn push_bounded(
    output: &mut String,
    value: &str,
    limit: usize,
    context: &'static str,
) -> Result<(), RuleModelError> {
    if output
        .len()
        .checked_add(value.len())
        .is_none_or(|length| length > limit)
    {
        return Err(limit_error(context, limit));
    }
    output.push_str(value);
    Ok(())
}

pub(crate) fn limit_error(context: &'static str, limit: usize) -> RuleModelError {
    RuleModelError::limit(
        context,
        format!("{context} exceeds the {limit}-byte rendered output limit"),
    )
}

#[cfg(test)]
mod tests;
