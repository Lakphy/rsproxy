use crate::RuleModelError;
use regex::{Regex, RegexBuilder};
use std::cell::RefCell;
use std::collections::VecDeque;

const REGEX_CACHE_CAPACITY: usize = 128;

thread_local! {
    static REGEX_CACHE: RefCell<VecDeque<CachedRegex>> = const { RefCell::new(VecDeque::new()) };
}

struct CachedRegex {
    pattern: String,
    case_insensitive: bool,
    regex: Regex,
}

struct ReplaceTransform {
    variable: String,
    pattern: String,
    case_insensitive: bool,
    replacement: String,
}

pub(super) fn apply_replace_transform(
    expression: &str,
    resolve: impl FnOnce(&str) -> String,
) -> Option<Result<String, RuleModelError>> {
    let parsed = match parse_replace_transform(expression) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return None,
        Err(error) => return Some(Err(error)),
    };
    let value = resolve(&parsed.variable);
    Some(replace_cached(
        &value,
        &parsed.pattern,
        parsed.case_insensitive,
        &parsed.replacement,
    ))
}

pub(crate) fn validate_template(input: &str) -> Result<(), RuleModelError> {
    let mut offset = 0;
    while let Some(relative_start) = input[offset..].find("${") {
        let start = offset + relative_start + 2;
        let end = find_template_end(input, start).ok_or_else(|| {
            RuleModelError::template("template variable", "unterminated template variable")
        })?;
        let expression = &input[start..end];
        if let Some(transform) = parse_replace_transform(expression)? {
            RegexBuilder::new(&transform.pattern)
                .case_insensitive(transform.case_insensitive)
                .build()
                .map_err(|source| RuleModelError::InvalidRegex {
                    context: "invalid template replace regex",
                    source: Box::new(source),
                })?;
        }
        offset = end + 1;
    }
    Ok(())
}

pub(super) fn find_template_end(input: &str, start: usize) -> Option<usize> {
    let mut braces = 0usize;
    let mut escaped = false;
    for (offset, character) in input[start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '{' {
            braces += 1;
        } else if character == '}' {
            if braces == 0 {
                return Some(start + offset);
            }
            braces -= 1;
        }
    }
    None
}

fn parse_replace_transform(expression: &str) -> Result<Option<ReplaceTransform>, RuleModelError> {
    let Some((variable, call)) = expression.split_once(".replace(") else {
        return Ok(None);
    };
    if variable.trim().is_empty() || !call.ends_with(')') {
        return Err(RuleModelError::template(
            "template replace",
            "template replace must be `${var.replace(/regex/, replacement)}`",
        ));
    }
    let arguments = &call[..call.len() - 1];
    if !arguments.starts_with('/') {
        return Err(RuleModelError::template(
            "template replace regex",
            "template replace regex must start with `/`",
        ));
    }
    let (end, case_insensitive, replacement_start) = replace_separator(arguments)?;
    Ok(Some(ReplaceTransform {
        variable: variable.trim().to_string(),
        pattern: unescape_slashes(&arguments[1..end]),
        case_insensitive,
        replacement: unquote(arguments[replacement_start..].trim()),
    }))
}

fn replace_separator(arguments: &str) -> Result<(usize, bool, usize), RuleModelError> {
    let mut escaped = false;
    for (index, character) in arguments.char_indices().skip(1) {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character != '/' {
            continue;
        }
        let tail = &arguments[index + 1..];
        let trimmed = tail.trim_start();
        let (case_insensitive, after_flags) = if let Some(after) = trimmed.strip_prefix('i') {
            (true, after.trim_start())
        } else {
            (false, trimmed)
        };
        if let Some(replacement) = after_flags.strip_prefix(',') {
            let start = arguments.len() - replacement.len();
            return Ok((index, case_insensitive, start));
        }
    }
    Err(RuleModelError::template(
        "template replace",
        "template replace must separate regex and replacement with a comma",
    ))
}

fn replace_cached(
    input: &str,
    pattern: &str,
    case_insensitive: bool,
    replacement: &str,
) -> Result<String, RuleModelError> {
    REGEX_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(index) = cache.iter().position(|entry| {
            entry.pattern == pattern && entry.case_insensitive == case_insensitive
        }) {
            let entry = cache
                .remove(index)
                .expect("located template regex cache entry must still exist");
            let output = entry.regex.replace_all(input, replacement).into_owned();
            cache.push_back(entry);
            return Ok(output);
        }
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(case_insensitive)
            .build()
            .map_err(|source| RuleModelError::InvalidRegex {
                context: "invalid template replace regex",
                source: Box::new(source),
            })?;
        let output = regex.replace_all(input, replacement).into_owned();
        if cache.len() == REGEX_CACHE_CAPACITY {
            cache.pop_front();
        }
        cache.push_back(CachedRegex {
            pattern: pattern.to_string(),
            case_insensitive,
            regex,
        });
        Ok(output)
    })
}

fn unescape_slashes(input: &str) -> String {
    input.replace("\\/", "/")
}

fn unquote(input: &str) -> String {
    let quoted = (input.starts_with('"') && input.ends_with('"'))
        || (input.starts_with('\'') && input.ends_with('\''));
    if quoted && input.len() >= 2 {
        input[1..input.len() - 1].to_string()
    } else {
        input.to_string()
    }
}
