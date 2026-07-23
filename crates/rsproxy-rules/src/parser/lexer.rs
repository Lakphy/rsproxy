use super::*;

/// One top-level rule token with a half-open UTF-8 byte range in its source line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuleToken {
    pub(crate) text: String,
    pub(crate) start: usize,
    pub(crate) end: usize,
}

pub(crate) fn tokenize(input: &str) -> Result<Vec<RuleToken>, RuleModelError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut token_start = None;
    let mut paren_depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;

    for (index, ch) in input.char_indices() {
        if token_start.is_none() && !ch.is_whitespace() {
            token_start = Some(index);
        }
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            current.push(ch);
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            current.push(ch);
            if ch == q {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            '(' => {
                paren_depth += 1;
                if paren_depth > MAX_PARSE_NESTING {
                    return Err(RuleModelError::limit(
                        "rule nesting",
                        format!("rule nesting exceeds {MAX_PARSE_NESTING} levels"),
                    ));
                }
                current.push(ch);
            }
            ')' => {
                if paren_depth == 0 {
                    return Err(RuleModelError::syntax("rule", "unmatched `)`"));
                }
                paren_depth -= 1;
                current.push(ch);
            }
            c if c.is_whitespace() && paren_depth == 0 => {
                if !current.is_empty() {
                    tokens.push(RuleToken {
                        text: std::mem::take(&mut current),
                        start: token_start.take().expect("non-empty token has a start"),
                        end: index,
                    });
                }
                token_start = None;
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return Err(RuleModelError::syntax("rule", "unterminated quote"));
    }
    if paren_depth != 0 {
        return Err(RuleModelError::syntax("rule", "unclosed `(`"));
    }
    if !current.is_empty() {
        tokens.push(RuleToken {
            text: current,
            start: token_start.expect("non-empty token has a start"),
            end: input.len(),
        });
    }
    Ok(tokens)
}
