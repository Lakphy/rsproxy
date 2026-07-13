use std::ffi::OsString;
use std::path::Path;

use super::model::{E2eReport, RequestMetrics, WhistleMetrics};
use super::{
    TargetCheck, TargetError, TargetOutcome, environment_text, failed_checks, invalid_report,
    number_environment, read_report,
};

const SCHEMA: &str = "rsproxy.e2e.performance/v1";
const DRIVER: &str = "oha";
const DEFAULT_MIN_RPS: f64 = 80_000.0;
const DEFAULT_MAX_ADDED_P50_US: f64 = 300.0;
const DEFAULT_MAX_ADDED_P99_US: f64 = 2_000.0;
const DEFAULT_MAX_EMPTY_RSS_KIB: f64 = 30_720.0;
const MIN_WHISTLE_SPEEDUP: f64 = 10.0;

pub(super) fn check(
    path: &Path,
    lookup: &dyn Fn(&str) -> Option<OsString>,
) -> Result<TargetOutcome, TargetError> {
    let report: E2eReport = read_report(path)?;
    validate(path, &report)?;
    let minimum_rps = number_environment("RSPROXY_PERF_MIN_RPS", DEFAULT_MIN_RPS, lookup)?;
    let maximum_p50 = number_environment(
        "RSPROXY_PERF_MAX_ADDED_P50_US",
        DEFAULT_MAX_ADDED_P50_US,
        lookup,
    )?;
    let maximum_p99 = number_environment(
        "RSPROXY_PERF_MAX_ADDED_P99_US",
        DEFAULT_MAX_ADDED_P99_US,
        lookup,
    )?;
    let maximum_empty_rss = number_environment(
        "RSPROXY_PERF_MAX_EMPTY_RSS_KIB",
        DEFAULT_MAX_EMPTY_RSS_KIB,
        lookup,
    )?;
    let require_whistle =
        environment_text("RSPROXY_PERF_REQUIRE_WHISTLE", lookup)?.is_some_and(|value| value == "1");

    let mut passed = Vec::new();
    let mut failed = Vec::new();
    record(
        report.proxy.requests_per_second >= minimum_rps,
        TargetCheck::new(
            "throughput",
            format!("{} rps", report.proxy.requests_per_second),
            format!(">= {minimum_rps} rps"),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.added_latency.p50_us < maximum_p50,
        TargetCheck::new(
            "added-latency-p50",
            format!("{} us", report.added_latency.p50_us),
            format!("< {maximum_p50} us"),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.added_latency.p99_us < maximum_p99,
        TargetCheck::new(
            "added-latency-p99",
            format!("{} us", report.added_latency.p99_us),
            format!("< {maximum_p99} us"),
        ),
        &mut passed,
        &mut failed,
    );
    record(
        report.memory.empty_rss_kib < maximum_empty_rss,
        TargetCheck::new(
            "empty-rss",
            format!("{} KiB", report.memory.empty_rss_kib),
            format!("< {maximum_empty_rss} KiB"),
        ),
        &mut passed,
        &mut failed,
    );
    if require_whistle {
        let speedup = report
            .whistle
            .and_then(|value| serde_json::from_value::<WhistleMetrics>(value).ok())
            .map(|whistle| whistle.speedup);
        record(
            speedup.is_some_and(|value| value >= MIN_WHISTLE_SPEEDUP),
            TargetCheck::new(
                "whistle-speedup",
                speedup.map_or_else(|| "missing x".to_owned(), |value| format!("{value} x")),
                format!(">= {MIN_WHISTLE_SPEEDUP}x"),
            ),
            &mut passed,
            &mut failed,
        );
    }

    if failed.is_empty() {
        Ok(TargetOutcome::new(
            passed,
            Some("All enabled e2e performance targets passed.".to_owned()),
        ))
    } else {
        Err(failed_checks(path, failed))
    }
}

fn validate(path: &Path, report: &E2eReport) -> Result<(), TargetError> {
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
    if report.requests <= 0.0 {
        return Err(invalid_report(
            path,
            "requests",
            "must be greater than zero",
        ));
    }
    if report.concurrency <= 0.0 {
        return Err(invalid_report(
            path,
            "concurrency",
            "must be greater than zero",
        ));
    }
    validate_request_metrics(path, "direct", &report.direct)?;
    validate_request_metrics(path, "proxy", &report.proxy)?;
    if report.added_latency.p50_us < 0.0 {
        return Err(invalid_report(
            path,
            "added_latency.p50_us",
            "must be non-negative",
        ));
    }
    if report.added_latency.p99_us < 0.0 {
        return Err(invalid_report(
            path,
            "added_latency.p99_us",
            "must be non-negative",
        ));
    }
    if report.memory.empty_rss_kib <= 0.0 {
        return Err(invalid_report(
            path,
            "memory.empty_rss_kib",
            "must be greater than zero",
        ));
    }
    Ok(())
}

fn validate_request_metrics(
    path: &Path,
    field: &str,
    metrics: &RequestMetrics,
) -> Result<(), TargetError> {
    if metrics.requests_per_second <= 0.0 {
        return Err(invalid_report(
            path,
            format!("{field}.requests_per_second"),
            "must be greater than zero",
        ));
    }
    if metrics.p50_us < 0.0 {
        return Err(invalid_report(
            path,
            format!("{field}.p50_us"),
            "must be non-negative",
        ));
    }
    if metrics.p99_us < metrics.p50_us {
        return Err(invalid_report(
            path,
            format!("{field}.p99_us"),
            format!("must be at least {}", metrics.p50_us),
        ));
    }
    if metrics.response_bytes <= 0.0 {
        return Err(invalid_report(
            path,
            format!("{field}.response_bytes"),
            "must be greater than zero",
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
