use super::*;

#[test]
fn log_settings_apply_filter_precedence_and_stable_defaults() {
    assert_eq!(
        LogSettings::from_values(None, None, None).unwrap(),
        LogSettings {
            filter: DEFAULT_FILTER.to_string(),
            format: LogFormat::Text,
        }
    );
    assert_eq!(
        LogSettings::from_values(Some("rsproxy=debug"), Some("warn"), Some("json")).unwrap(),
        LogSettings {
            filter: "rsproxy=debug".to_string(),
            format: LogFormat::Json,
        }
    );
    assert_eq!(
        LogSettings::from_values(Some("  "), Some("error"), Some("compact")).unwrap(),
        LogSettings {
            filter: "error".to_string(),
            format: LogFormat::Text,
        }
    );
}

#[test]
fn log_settings_reject_unknown_output_formats() {
    let error = LogSettings::from_values(None, None, Some("yaml")).unwrap_err();
    assert!(error.contains("RSPROXY_LOG_FORMAT"));
    assert!(error.contains("text or json"));
}
