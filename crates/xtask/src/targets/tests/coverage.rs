use super::super::{TargetCommand, TargetError};
use super::support::{ReportFile, coverage_report, failed_labels, run};
use serde_json::json;

#[test]
fn coverage_contract_accepts_boundaries_and_rejects_below_target() {
    let passing = ReportFile::new(&coverage_report(8_500.0, 9_500.0));
    let outcome = run(
        TargetCommand::Coverage {
            report: passing.path().to_path_buf(),
        },
        &[],
    )
    .expect("coverage boundaries pass");
    assert_eq!(outcome.checks().len(), 2);

    let workspace_failure = ReportFile::new(&coverage_report(8_499.0, 9_500.0));
    let error = run(
        TargetCommand::Coverage {
            report: workspace_failure.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("workspace coverage below 85% fails");
    assert_eq!(failed_labels(&error), ["workspace-lines"]);

    let rules_failure = ReportFile::new(&coverage_report(8_500.0, 9_499.0));
    let error = run(
        TargetCommand::Coverage {
            report: rules_failure.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("rules coverage below 95% fails");
    assert_eq!(failed_labels(&error), ["rules-lines"]);
}

#[test]
fn coverage_thresholds_honor_environment_overrides() {
    let report = ReportFile::new(&coverage_report(8_000.0, 9_000.0));
    run(
        TargetCommand::Coverage {
            report: report.path().to_path_buf(),
        },
        &[
            ("RSPROXY_COVERAGE_MIN_WORKSPACE", "80"),
            ("RSPROXY_COVERAGE_MIN_RULES", "90"),
        ],
    )
    .expect("environment overrides lower both thresholds");

    let error = run(
        TargetCommand::Coverage {
            report: report.path().to_path_buf(),
        },
        &[("RSPROXY_COVERAGE_MIN_WORKSPACE", "not-a-number")],
    )
    .expect_err("invalid numeric environment value fails");
    assert!(matches!(
        error,
        TargetError::InvalidEnvironment {
            name: "RSPROXY_COVERAGE_MIN_WORKSPACE",
            ..
        }
    ));
}

#[test]
fn coverage_schema_relations_report_path_and_field() {
    let mut report = coverage_report(8_500.0, 9_500.0);
    report["workspace"]["covered"] = 10_001.into();
    let report = ReportFile::new(&report);
    let error = run(
        TargetCommand::Coverage {
            report: report.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("covered lines cannot exceed total lines");
    let message = error.to_string();
    assert!(message.contains(&report.path().display().to_string()));
    assert!(message.contains("workspace.covered"));
}

#[test]
fn coverage_rejects_invalid_schema_and_metric_domains() {
    for (pointer, value, field) in [
        ("/schema", json!("other"), "schema"),
        ("/workspace/lines", json!(0), "workspace.lines"),
        ("/workspace/covered", json!(-1), "workspace.covered"),
        ("/workspace/percent", json!(101), "workspace.percent"),
        ("/rules/lines", json!(0), "rules.lines"),
    ] {
        let mut report = coverage_report(8_500.0, 9_500.0);
        *report.pointer_mut(pointer).expect("fixture field") = value;
        let report = ReportFile::new(&report);
        let error = run(
            TargetCommand::Coverage {
                report: report.path().to_path_buf(),
            },
            &[],
        )
        .expect_err("invalid coverage report must fail");
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn both_failed_coverage_checks_render_with_a_separator() {
    let report = ReportFile::new(&coverage_report(0.0, 0.0));
    let error = run(
        TargetCommand::Coverage {
            report: report.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("both coverage targets must fail");
    let rendered = error.to_string();
    assert!(rendered.contains("workspace-lines observed="));
    assert!(rendered.contains("; rules-lines observed="));
}
