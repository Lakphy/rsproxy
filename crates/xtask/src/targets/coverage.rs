use std::ffi::OsString;
use std::path::Path;

use super::model::{CoverageMetric, CoverageReport};
use super::{
    TargetCheck, TargetError, TargetOutcome, failed_checks, invalid_report, number_environment,
    read_report,
};

const SCHEMA: &str = "rsproxy.coverage/v1";
const DEFAULT_MIN_WORKSPACE: f64 = 85.0;
const DEFAULT_MIN_RULES: f64 = 95.0;

pub(super) fn check(
    path: &Path,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<TargetOutcome, TargetError> {
    let report: CoverageReport = read_report(path)?;
    validate(path, &report)?;
    let minimum_workspace = number_environment(
        "RSPROXY_COVERAGE_MIN_WORKSPACE",
        DEFAULT_MIN_WORKSPACE,
        lookup,
    )?;
    let minimum_rules =
        number_environment("RSPROXY_COVERAGE_MIN_RULES", DEFAULT_MIN_RULES, lookup)?;

    let mut passed = Vec::new();
    let mut failed = Vec::new();
    record(
        report.workspace.percent >= minimum_workspace,
        TargetCheck::new(
            "workspace-lines",
            format!("{}%", report.workspace.percent),
            format!(">={minimum_workspace}%"),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.rules.percent >= minimum_rules,
        TargetCheck::new(
            "rules-lines",
            format!("{}%", report.rules.percent),
            format!(">={minimum_rules}%"),
        ),
        &mut passed,
        &mut failed,
    );
    if failed.is_empty() {
        Ok(TargetOutcome::new(passed, None))
    } else {
        Err(failed_checks(path, failed))
    }
}

fn validate(path: &Path, report: &CoverageReport) -> Result<(), TargetError> {
    if report.schema != SCHEMA {
        return Err(invalid_report(
            path,
            "schema",
            format!("must equal {SCHEMA:?}"),
        ));
    }
    validate_metric(path, "workspace", &report.workspace)?;
    validate_metric(path, "rules", &report.rules)
}

fn validate_metric(path: &Path, field: &str, metric: &CoverageMetric) -> Result<(), TargetError> {
    if metric.lines <= 0.0 {
        return Err(invalid_report(
            path,
            format!("{field}.lines"),
            "must be greater than zero",
        ));
    }
    if metric.covered < 0.0 || metric.covered > metric.lines {
        return Err(invalid_report(
            path,
            format!("{field}.covered"),
            format!("must be between zero and {}", metric.lines),
        ));
    }
    if !(0.0..=100.0).contains(&metric.percent) {
        return Err(invalid_report(
            path,
            format!("{field}.percent"),
            "must be between zero and 100",
        ));
    }
    Ok(())
}

fn record(
    condition: bool,
    check: TargetCheck,
    passed: &mut Vec<TargetCheck>,
    failed: &mut Vec<TargetCheck>,
) {
    if condition {
        passed.push(check);
    } else {
        failed.push(check);
    }
}
