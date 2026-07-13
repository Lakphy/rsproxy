use std::ffi::OsString;
use std::path::Path;

use super::model::{CriterionMetric, CriterionRegressionReport, CriterionTargetReport};
use super::{
    RegressionFailure, RegressionFailures, TargetCheck, TargetError, TargetOutcome, failed_checks,
    invalid_report, number_environment, read_report,
};

const SCHEMA: &str = "rsproxy.criterion/v1";
const UNIT: &str = "nanoseconds";
const TLS_METRIC: &str = "mitm_certificate/cached_tls_handshake";
const DEFAULT_MAX_TLS_HANDSHAKE_NS: f64 = 3_000_000.0;

pub(super) fn check_target(
    path: &Path,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<TargetOutcome, TargetError> {
    let report: CriterionTargetReport = read_report(path)?;
    validate_envelope(path, &report.schema, &report.unit)?;
    let value = report
        .metrics
        .get(TLS_METRIC)
        .ok_or_else(|| invalid_report(path, format!("metrics.{TLS_METRIC}"), "is required"))?;
    let metric: CriterionMetric = serde_json::from_value(value.clone()).map_err(|source| {
        invalid_report(
            path,
            format!("metrics.{TLS_METRIC}"),
            format!("must contain numeric mean_ns/lower_ns/upper_ns: {source}"),
        )
    })?;
    validate_target_metric(path, &metric)?;

    let maximum = number_environment(
        "RSPROXY_PERF_MAX_TLS_HANDSHAKE_NS",
        DEFAULT_MAX_TLS_HANDSHAKE_NS,
        lookup,
    )?;
    let check = TargetCheck::new(
        "cached-tls-handshake",
        format!("{}ns", metric.upper_ns),
        format!("<{maximum}ns"),
    );
    if metric.upper_ns < maximum {
        Ok(TargetOutcome::new(vec![check], None))
    } else {
        Err(failed_checks(path, vec![check]))
    }
}

pub(super) fn check_regression(
    baseline_path: &Path,
    current_path: &Path,
    tolerance_percent: f64,
) -> Result<TargetOutcome, TargetError> {
    let baseline: CriterionRegressionReport = read_report(baseline_path)?;
    let current: CriterionRegressionReport = read_report(current_path)?;
    validate_regression_report(baseline_path, &baseline)?;
    validate_regression_report(current_path, &current)?;

    let missing = baseline
        .metrics
        .keys()
        .filter(|metric| !current.metrics.contains_key(*metric))
        .cloned()
        .collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(TargetError::MissingMetrics {
            path: current_path.to_path_buf(),
            metrics: missing.join(", "),
        });
    }

    let mut regressions = Vec::new();
    for (name, baseline_metric) in &baseline.metrics {
        let current_metric = &current.metrics[name];
        let maximum = baseline_metric.mean_ns * (1.0 + tolerance_percent / 100.0);
        if current_metric.mean_ns > maximum {
            regressions.push(RegressionFailure {
                metric: name.clone(),
                baseline_ns: baseline_metric.mean_ns,
                current_ns: current_metric.mean_ns,
                change_percent: (current_metric.mean_ns / baseline_metric.mean_ns - 1.0) * 100.0,
                tolerance_percent,
            });
        }
    }
    if !regressions.is_empty() {
        return Err(TargetError::Regressions {
            path: current_path.to_path_buf(),
            failures: RegressionFailures(regressions),
        });
    }

    Ok(TargetOutcome::new(
        Vec::new(),
        Some(format!(
            "Compared {} Criterion metrics; no regression exceeded {}%.",
            baseline.metrics.len(),
            tolerance_percent
        )),
    ))
}

fn validate_envelope(path: &Path, schema: &str, unit: &str) -> Result<(), TargetError> {
    if schema != SCHEMA {
        return Err(invalid_report(
            path,
            "schema",
            format!("must equal {SCHEMA:?}"),
        ));
    }
    if unit != UNIT {
        return Err(invalid_report(path, "unit", format!("must equal {UNIT:?}")));
    }
    Ok(())
}

fn validate_target_metric(path: &Path, metric: &CriterionMetric) -> Result<(), TargetError> {
    for (field, value) in [
        ("mean_ns", metric.mean_ns),
        ("lower_ns", metric.lower_ns),
        ("upper_ns", metric.upper_ns),
    ] {
        if value <= 0.0 {
            return Err(invalid_report(
                path,
                format!("metrics.{TLS_METRIC}.{field}"),
                "must be greater than zero",
            ));
        }
    }
    if metric.lower_ns > metric.mean_ns {
        return Err(invalid_report(
            path,
            format!("metrics.{TLS_METRIC}.lower_ns"),
            "must not exceed mean_ns",
        ));
    }
    if metric.mean_ns > metric.upper_ns {
        return Err(invalid_report(
            path,
            format!("metrics.{TLS_METRIC}.upper_ns"),
            "must not be less than mean_ns",
        ));
    }
    Ok(())
}

fn validate_regression_report(
    path: &Path,
    report: &CriterionRegressionReport,
) -> Result<(), TargetError> {
    validate_envelope(path, &report.schema, &report.unit)?;
    if report.metrics.is_empty() {
        return Err(invalid_report(
            path,
            "metrics",
            "must contain at least one metric",
        ));
    }
    Ok(())
}
