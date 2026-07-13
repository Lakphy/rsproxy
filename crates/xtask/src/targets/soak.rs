use std::ffi::OsString;
use std::path::Path;

use super::model::{GrowthMetric, SoakReport};
use super::{
    TargetCheck, TargetError, TargetOutcome, environment_text, failed_checks, integer_environment,
    invalid_report, non_negative_integer, number_environment, read_report,
};

const SCHEMA: &str = "rsproxy.soak/v1";
const DRIVER: &str = "oha";

struct Thresholds {
    maximum_rss_growth_kib: f64,
    maximum_fd_end_growth: f64,
    maximum_fd_peak_growth: f64,
    minimum_rate_ratio: f64,
    minimum_elapsed_seconds: f64,
    minimum_requests: f64,
    minimum_samples: f64,
    maximum_rss_last_half_slope: f64,
}

pub(super) fn check(
    path: &Path,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<TargetOutcome, TargetError> {
    let report: SoakReport = read_report(path)?;
    validate(path, &report)?;
    let thresholds = Thresholds::from_environment(report.configured.concurrency, lookup)?;
    let mut passed = Vec::new();
    let mut failed = Vec::new();

    record(
        report.load.requests_per_second >= report.configured.qps * thresholds.minimum_rate_ratio,
        TargetCheck::new(
            "request-rate",
            format!("{} rps", report.load.requests_per_second),
            format!(">= configured qps * {}", thresholds.minimum_rate_ratio),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.elapsed_seconds >= thresholds.minimum_elapsed_seconds,
        TargetCheck::new(
            "elapsed-time",
            format!("{} seconds", report.elapsed_seconds),
            format!(">= {} seconds", thresholds.minimum_elapsed_seconds),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.load.requests >= thresholds.minimum_requests,
        TargetCheck::new(
            "request-volume",
            report.load.requests.to_string(),
            format!(">= {}", thresholds.minimum_requests),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.process.samples >= thresholds.minimum_samples,
        TargetCheck::new(
            "sample-depth",
            report.process.samples.to_string(),
            format!(">= {}", thresholds.minimum_samples),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.load.success_rate == 1.0
            && report.load.errors == 0.0
            && report.load.status_200 == report.load.requests
            && report.load.response_bytes == report.load.requests * 1024.0,
        TargetCheck::new(
            "load-correctness",
            format!("{} success", report.load.success_rate),
            "exact 200/bytes and zero errors",
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.process.rss_kib.growth.peak_growth <= thresholds.maximum_rss_growth_kib,
        TargetCheck::new(
            "rss-peak-growth",
            format!("{} KiB", report.process.rss_kib.growth.peak_growth),
            format!("<= {} KiB", thresholds.maximum_rss_growth_kib),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.process.rss_kib.growth.end_growth <= thresholds.maximum_rss_growth_kib,
        TargetCheck::new(
            "rss-end-growth",
            format!("{} KiB", report.process.rss_kib.growth.end_growth),
            format!("<= {} KiB", thresholds.maximum_rss_growth_kib),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.process.rss_kib.last_half_slope_kib_per_hour
            <= thresholds.maximum_rss_last_half_slope,
        TargetCheck::new(
            "rss-steady-slope",
            format!(
                "{} KiB/hour",
                report.process.rss_kib.last_half_slope_kib_per_hour
            ),
            format!("<= {} KiB/hour", thresholds.maximum_rss_last_half_slope),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.process.fds.peak_growth <= thresholds.maximum_fd_peak_growth,
        TargetCheck::new(
            "fd-peak-growth",
            report.process.fds.peak_growth.to_string(),
            format!("<= {}", thresholds.maximum_fd_peak_growth),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.process.fds.end_growth <= thresholds.maximum_fd_end_growth,
        TargetCheck::new(
            "fd-end-growth",
            report.process.fds.end_growth.to_string(),
            format!("<= {}", thresholds.maximum_fd_end_growth),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.rules.loaded == report.configured.rules,
        TargetCheck::new(
            "rules-loaded",
            report.rules.loaded.to_string(),
            "configured rule count",
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.trace.pending_sessions == 0.0
            && report.trace.incomplete_sessions == 0.0
            && report.trace.orphan_events == 0.0
            && report.trace.queue_bytes == 0.0,
        TargetCheck::new(
            "trace-drained",
            format!(
                "pending={} orphan={}",
                report.trace.pending_sessions, report.trace.orphan_events
            ),
            "zero pending/incomplete/orphan/queue bytes",
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.trace.queue_dropped == 0.0
            && report.trace.queue_memory_dropped == 0.0
            && report.trace.spill_errors == 0.0,
        TargetCheck::new(
            "trace-loss",
            format!(
                "queue_dropped={} spill_errors={}",
                report.trace.queue_dropped, report.trace.spill_errors
            ),
            "zero queue/memory/spill errors",
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.trace.sessions <= report.trace.max_sessions
            && report.trace.total_memory_bytes <= report.trace.memory_budget_bytes,
        TargetCheck::new(
            "trace-bounds",
            format!(
                "sessions={} memory={}",
                report.trace.sessions, report.trace.total_memory_bytes
            ),
            "configured session and memory budgets",
        ),
        &mut passed,
        &mut failed,
    );

    if failed.is_empty() {
        Ok(TargetOutcome::new(
            passed,
            Some("Soak stability targets passed.".to_owned()),
        ))
    } else {
        Err(failed_checks(path, failed))
    }
}

impl Thresholds {
    fn from_environment(
        concurrency: f64,
        lookup: &dyn Fn(&str) -> Option<OsString>,
    ) -> Result<Self, TargetError> {
        let maximum_fd_end_growth =
            match environment_text("RSPROXY_SOAK_MAX_FD_END_GROWTH", lookup)? {
                Some(value) => non_negative_integer("RSPROXY_SOAK_MAX_FD_END_GROWTH", value)?,
                None => integer_environment("RSPROXY_SOAK_MAX_FD_GROWTH", 16, lookup)?,
            } as f64;
        let fd_peak_headroom =
            integer_environment("RSPROXY_SOAK_FD_PEAK_HEADROOM", 16, lookup)? as f64;
        let maximum_fd_peak_growth =
            match environment_text("RSPROXY_SOAK_MAX_FD_PEAK_GROWTH", lookup)? {
                Some(value) => {
                    non_negative_integer("RSPROXY_SOAK_MAX_FD_PEAK_GROWTH", value)? as f64
                }
                None => concurrency * 2.0 + fd_peak_headroom,
            };
        Ok(Self {
            maximum_rss_growth_kib: number_environment(
                "RSPROXY_SOAK_MAX_RSS_GROWTH_KIB",
                32_768.0,
                lookup,
            )?,
            maximum_fd_end_growth,
            maximum_fd_peak_growth,
            minimum_rate_ratio: number_environment("RSPROXY_SOAK_MIN_RATE_RATIO", 0.90, lookup)?,
            minimum_elapsed_seconds: number_environment(
                "RSPROXY_SOAK_MIN_ELAPSED_SECONDS",
                5_400.0,
                lookup,
            )?,
            minimum_requests: number_environment("RSPROXY_SOAK_MIN_REQUESTS", 5_000_000.0, lookup)?,
            minimum_samples: number_environment("RSPROXY_SOAK_MIN_SAMPLES", 90.0, lookup)?,
            maximum_rss_last_half_slope: number_environment(
                "RSPROXY_SOAK_MAX_RSS_LAST_HALF_SLOPE_KIB_PER_HOUR",
                1_024.0,
                lookup,
            )?,
        })
    }
}

fn validate(path: &Path, report: &SoakReport) -> Result<(), TargetError> {
    if report.schema != SCHEMA {
        return Err(invalid_report(
            path,
            "schema",
            format!("must equal {SCHEMA:?}"),
        ));
    }
    if report.driver != DRIVER {
        return Err(invalid_report(
            path,
            "driver",
            format!("must equal {DRIVER:?}"),
        ));
    }
    if report.duration.is_empty() {
        return Err(invalid_report(path, "duration", "must not be empty"));
    }
    for (field, value) in [
        ("configured.qps", report.configured.qps),
        ("configured.concurrency", report.configured.concurrency),
        ("configured.rules", report.configured.rules),
        ("load.requests", report.load.requests),
        ("load.requests_per_second", report.load.requests_per_second),
    ] {
        if value <= 0.0 {
            return Err(invalid_report(path, field, "must be greater than zero"));
        }
    }
    if report.process.samples < 2.0 {
        return Err(invalid_report(
            path,
            "process.samples",
            "must be at least two",
        ));
    }
    for (field, value) in [
        (
            "process.rss_kib.slope_kib_per_hour",
            report.process.rss_kib.slope_kib_per_hour,
        ),
        (
            "process.rss_kib.last_half_slope_kib_per_hour",
            report.process.rss_kib.last_half_slope_kib_per_hour,
        ),
    ] {
        if !value.is_finite() {
            return Err(invalid_report(path, field, "must be a finite number"));
        }
    }
    validate_growth(path, "process.rss_kib", &report.process.rss_kib.growth)?;
    validate_growth(path, "process.fds", &report.process.fds)
}

fn validate_growth(path: &Path, field: &str, metric: &GrowthMetric) -> Result<(), TargetError> {
    if metric.start < 0.0 {
        return Err(invalid_report(
            path,
            format!("{field}.start"),
            "must be non-negative",
        ));
    }
    if metric.end < 0.0 {
        return Err(invalid_report(
            path,
            format!("{field}.end"),
            "must be non-negative",
        ));
    }
    if metric.maximum < metric.start {
        return Err(invalid_report(
            path,
            format!("{field}.max"),
            "must be at least start",
        ));
    }
    if metric.end_growth < 0.0 {
        return Err(invalid_report(
            path,
            format!("{field}.end_growth"),
            "must be non-negative",
        ));
    }
    if metric.peak_growth < metric.end_growth {
        return Err(invalid_report(
            path,
            format!("{field}.peak_growth"),
            "must be at least end_growth",
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
