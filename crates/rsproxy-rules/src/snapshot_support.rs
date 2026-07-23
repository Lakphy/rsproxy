use super::*;
use crate::template::now_millis;

pub(super) fn push_parse_diagnostic(errors: &mut Vec<RuleError>, error: RuleError) -> bool {
    if errors.len() < MAX_RULE_DIAGNOSTICS.saturating_sub(1) {
        errors.push(error);
        return false;
    }
    if errors.len() < MAX_RULE_DIAGNOSTICS {
        errors.push(RuleError {
            code: RuleErrorCode::Syntax,
            group: error.group,
            line: error.line,
            span: error.span,
            message: format!(
                "diagnostic limit of {MAX_RULE_DIAGNOSTICS} reached; remaining source was not parsed"
            ),
        });
    }
    true
}

pub(super) fn next_ruleset_version() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    static LAST_VERSION: AtomicU64 = AtomicU64::new(0);
    let wall_clock = now_millis().max(1);
    let mut current = LAST_VERSION.load(Ordering::Relaxed);
    loop {
        let next = wall_clock.max(current.saturating_add(1));
        match LAST_VERSION.compare_exchange_weak(
            current,
            next,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => return next,
            Err(observed) => current = observed,
        }
    }
}
