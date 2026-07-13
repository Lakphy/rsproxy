use super::*;

#[test]
fn passthrough_precedence_is_explicit() {
    assert_eq!(
        passthrough_reason(true, true, false, true),
        Some(PassthroughReason::Disabled)
    );
    assert_eq!(
        passthrough_reason(false, true, false, true),
        Some(PassthroughReason::RuleBypass)
    );
    assert_eq!(
        passthrough_reason(false, false, false, true),
        Some(PassthroughReason::MissingCa)
    );
    assert_eq!(
        passthrough_reason(false, false, true, true),
        Some(PassthroughReason::RememberedFailure)
    );
    assert_eq!(passthrough_reason(false, false, true, false), None);
}

#[test]
fn passthrough_reasons_have_stable_trace_flags() {
    assert_eq!(PassthroughReason::Disabled.flag(), "no-mitm");
    assert_eq!(PassthroughReason::RuleBypass.flag(), "bypass");
    assert_eq!(PassthroughReason::MissingCa.flag(), "no-ca");
    assert_eq!(
        PassthroughReason::RememberedFailure.flag(),
        "mitm-fallback-cache-hit"
    );
}
