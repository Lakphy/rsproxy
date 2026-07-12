use super::*;

#[test]
fn parse_errors_have_stable_stage_codes() {
    for (rule, expected) in [
        (
            "example.test req.header(\"unterminated)",
            RuleErrorCode::Syntax,
        ),
        (":0 status(200)", RuleErrorCode::Matcher),
        ("example.test unknown()", RuleErrorCode::Action),
        (
            "example.test status(200) when unknown()",
            RuleErrorCode::Condition,
        ),
        ("example.test status(200) @unknown", RuleErrorCode::Property),
    ] {
        let errors = RuleSet::parse("contract", rule).unwrap_err();
        assert_eq!(errors.len(), 1, "{rule}");
        assert_eq!(errors[0].code, expected, "{rule}");
        assert_eq!(errors[0].code.as_str(), expected.as_str());
        assert_eq!(errors[0].group, "contract");
        assert_eq!(errors[0].line, 1);
        assert!(!errors[0].message.is_empty());
    }
}
