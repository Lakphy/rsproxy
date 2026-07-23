use crate::{
    RULE_LANGUAGE_HEADER, canonical_action_name, canonical_condition_name, parser::strip_comment,
};

/// Upgrades an unversioned/v2 rule source to canonical v3 syntax.
///
/// The migration adds or replaces the language directive and rewrites known
/// action/condition aliases only when they occur as call names. Quoted values
/// and regex bodies are preserved byte-for-byte.
pub fn migrate_rule_source_v3(source: &str) -> String {
    let had_trailing_newline = source.ends_with('\n');
    let mut lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    let first_effective = lines
        .iter()
        .position(|line| strip_comment(line).is_some_and(|line| !line.trim().is_empty()));

    match first_effective {
        Some(index) if is_language_directive(&lines[index]) => {
            lines[index] = replace_language_directive(&lines[index]);
        }
        Some(index) => lines.insert(index, RULE_LANGUAGE_HEADER.to_string()),
        None => lines.push(RULE_LANGUAGE_HEADER.to_string()),
    }

    for line in &mut lines {
        if !is_language_directive(line) {
            *line = canonicalize_call_names(line);
        }
    }

    let mut migrated = lines.join("\n");
    if had_trailing_newline || source.is_empty() {
        migrated.push('\n');
    }
    migrated
}

fn is_language_directive(line: &str) -> bool {
    let Some(effective) = strip_comment(line) else {
        return false;
    };
    let effective = effective.trim();
    effective == "@language"
        || effective
            .strip_prefix("@language")
            .is_some_and(|suffix| suffix.chars().next().is_some_and(char::is_whitespace))
}

fn replace_language_directive(line: &str) -> String {
    let leading_bytes = line.len() - line.trim_start().len();
    let leading = &line[..leading_bytes];
    let Some(comment_start) = line.find('#') else {
        return format!("{leading}{RULE_LANGUAGE_HEADER}");
    };
    let directive = &line[leading_bytes..comment_start];
    let gap_start = directive.trim_end().len() + leading_bytes;
    format!("{leading}{RULE_LANGUAGE_HEADER}{}", &line[gap_start..])
}

fn canonicalize_call_names(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut index = 0usize;
    let mut quote = None;
    let mut escaped = false;
    let mut regex_end = None;

    while index < line.len() {
        let character = line[index..]
            .chars()
            .next()
            .expect("index remains on a UTF-8 boundary");
        let width = character.len_utf8();

        if let Some(end) = regex_end {
            output.push(character);
            index += width;
            if index - width == end {
                regex_end = None;
            }
            continue;
        }
        if escaped {
            output.push(character);
            escaped = false;
            index += width;
            continue;
        }
        if character == '\\' {
            output.push(character);
            escaped = true;
            index += width;
            continue;
        }
        if let Some(delimiter) = quote {
            output.push(character);
            if character == delimiter {
                quote = None;
            }
            index += width;
            continue;
        }
        if matches!(character, '\'' | '"') {
            quote = Some(character);
            output.push(character);
            index += width;
            continue;
        }
        if character == '#' {
            output.push_str(&line[index..]);
            break;
        }
        if character == '/'
            && begins_regex(line, index)
            && let Some(end) = regex_end_at_boundary(line, index)
        {
            regex_end = Some(end);
            output.push(character);
            index += width;
            continue;
        }
        if character.is_ascii_alphabetic() {
            let start = index;
            index += width;
            while index < line.len() {
                let next = line[index..]
                    .chars()
                    .next()
                    .expect("index remains on a UTF-8 boundary");
                if !(next.is_ascii_alphanumeric() || matches!(next, '.' | '_' | '-')) {
                    break;
                }
                index += next.len_utf8();
            }
            let name = &line[start..index];
            if line[index..].starts_with('(') {
                let canonical = canonical_action_name(name)
                    .or_else(|| canonical_condition_name(name))
                    .unwrap_or(name);
                output.push_str(canonical);
            } else {
                output.push_str(name);
            }
            continue;
        }
        output.push(character);
        index += width;
    }
    output
}

fn begins_regex(line: &str, index: usize) -> bool {
    line[..index]
        .chars()
        .rev()
        .find(|character| !character.is_whitespace())
        .is_none_or(|character| matches!(character, '(' | ',' | '=' | '~' | '!'))
}

fn regex_end_at_boundary(line: &str, start: usize) -> Option<usize> {
    let mut escaped = false;
    let mut character_class = false;
    for (offset, character) in line[start + 1..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        if character == '[' {
            character_class = true;
            continue;
        }
        if character == ']' && character_class {
            character_class = false;
            continue;
        }
        if character != '/' || character_class {
            continue;
        }
        let end = start + 1 + offset;
        let next = line[end + 1..].chars().next();
        if next.is_none_or(|character| character.is_whitespace() || matches!(character, ',' | ')'))
        {
            return Some(end);
        }
    }
    None
}
