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

#[test]
fn internal_parser_errors_retain_typed_numeric_and_regex_sources() {
    let duration = parse_duration_ms("badms").unwrap_err();
    assert!(matches!(
        duration,
        RuleModelError::InvalidInteger {
            context: "duration",
            ..
        }
    ));

    let speed = parse_speed_bps("bad").unwrap_err();
    assert!(matches!(
        speed,
        RuleModelError::InvalidFloat {
            context: "speed",
            ..
        }
    ));

    let regex = template::transform::validate_template("${x.replace(/[/, value)}").unwrap_err();
    assert!(matches!(
        regex,
        RuleModelError::InvalidRegex {
            context: "invalid template replace regex",
            ..
        }
    ));
}
