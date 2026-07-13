use std::fs;
use std::path::PathBuf;

use clap::{Parser, Subcommand};

use super::super::{TargetCheck, TargetCommand, TargetError, TargetOutcome, TargetsArgs};
use super::support::{ReportFile, coverage_report, criterion_report, run};

#[derive(Debug, Parser)]
struct Harness {
    #[command(subcommand)]
    command: HarnessCommand,
}

#[derive(Debug, Subcommand)]
enum HarnessCommand {
    Targets(TargetsArgs),
}

#[test]
fn clap_surface_supports_all_target_kinds_and_regression_default() {
    for kind in ["coverage", "criterion", "e2e", "soak"] {
        Harness::try_parse_from(["xtask", "targets", kind, "report.json"])
            .expect("single-report target command parses");
    }
    let parsed = Harness::try_parse_from([
        "xtask",
        "targets",
        "regression",
        "baseline.json",
        "current.json",
    ])
    .expect("regression command parses with default tolerance");
    let HarnessCommand::Targets(TargetsArgs {
        command: TargetCommand::Regression {
            tolerance_percent, ..
        },
    }) = parsed.command
    else {
        panic!("expected regression arguments");
    };
    assert_eq!(tolerance_percent, 10.0);
}

#[test]
fn read_and_parse_errors_retain_report_path() {
    let missing = PathBuf::from("/definitely/missing/rsproxy-target-report.json");
    let error = run(
        TargetCommand::Coverage {
            report: missing.clone(),
        },
        &[],
    )
    .expect_err("missing report fails");
    assert!(matches!(error, TargetError::Read { .. }));
    assert!(error.to_string().contains(&missing.display().to_string()));

    let report = ReportFile::new(&coverage_report(8_500.0, 9_500.0));
    fs::write(report.path(), b"{not json").expect("write malformed report");
    let error = run(
        TargetCommand::Coverage {
            report: report.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("malformed JSON fails");
    assert!(matches!(error, TargetError::Parse { .. }));
    assert!(
        error
            .to_string()
            .contains(&report.path().display().to_string())
    );
}

#[test]
fn floating_environment_thresholds_reject_non_finite_values() {
    let report = ReportFile::new(&coverage_report(8_500.0, 9_500.0));
    for value in ["NaN", "inf", "1e999"] {
        let error = run(
            TargetCommand::Coverage {
                report: report.path().to_path_buf(),
            },
            &[("RSPROXY_COVERAGE_MIN_WORKSPACE", value)],
        )
        .expect_err("non-finite threshold fails");
        assert!(matches!(error, TargetError::InvalidEnvironment { .. }));
    }
}

#[test]
fn regression_tolerance_must_be_finite_and_non_negative() {
    let baseline = ReportFile::new(&criterion_report("sample", 100.0));
    let current = ReportFile::new(&criterion_report("sample", 100.0));
    for tolerance_percent in [-1.0, f64::NAN, f64::INFINITY] {
        let error = run(
            TargetCommand::Regression {
                baseline: baseline.path().to_path_buf(),
                current: current.path().to_path_buf(),
                tolerance_percent,
            },
            &[],
        )
        .expect_err("invalid tolerance fails");
        assert!(matches!(error, TargetError::InvalidArgument { .. }));
    }
}

#[test]
fn successful_outcome_is_ready_for_main_to_print() {
    let report = ReportFile::new(&coverage_report(8_500.0, 9_500.0));
    let outcome = run(
        TargetCommand::Coverage {
            report: report.path().to_path_buf(),
        },
        &[],
    )
    .expect("valid report passes");
    let rendered = outcome.to_string();
    assert!(rendered.contains("PASS workspace-lines"));
    assert!(rendered.contains("PASS rules-lines"));
}

#[test]
fn outcome_accessors_and_rendering_include_checks_and_summary() {
    let outcome = TargetOutcome::new(
        vec![
            TargetCheck::new("first", "1", ">=1"),
            TargetCheck::new("second", "2", ">=2"),
        ],
        Some("all done".to_owned()),
    );
    assert_eq!(outcome.summary(), Some("all done"));
    assert_eq!(
        outcome.to_string(),
        "PASS first observed=1 target=>=1\nPASS second observed=2 target=>=2\nall done"
    );
}

#[test]
fn empty_environment_value_uses_the_default_threshold() {
    let report = ReportFile::new(&coverage_report(8_500.0, 9_500.0));
    run(
        TargetCommand::Coverage {
            report: report.path().to_path_buf(),
        },
        &[("RSPROXY_COVERAGE_MIN_WORKSPACE", "")],
    )
    .expect("an empty environment override is absent");
}
