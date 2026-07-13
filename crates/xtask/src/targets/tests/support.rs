use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tempfile::TempDir;

use super::super::{TargetCommand, TargetError, TargetOutcome, TargetsArgs, run_with_environment};

pub(super) struct ReportFile {
    _directory: TempDir,
    path: PathBuf,
}

impl ReportFile {
    pub(super) fn new(report: &Value) -> Self {
        let directory = tempfile::tempdir().expect("create report fixture");
        let path = directory.path().join("report.json");
        let fixture = Self {
            _directory: directory,
            path,
        };
        fixture.write(report);
        fixture
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn write(&self, report: &Value) {
        fs::write(
            &self.path,
            serde_json::to_vec_pretty(report).expect("serialize report"),
        )
        .expect("write report");
    }
}

pub(super) fn run(
    command: TargetCommand,
    environment: &[(&str, &str)],
) -> Result<TargetOutcome, TargetError> {
    let environment = environment
        .iter()
        .map(|(name, value)| ((*name).to_owned(), OsString::from(value)))
        .collect::<BTreeMap<_, _>>();
    run_with_environment(&TargetsArgs { command }, &|name| {
        environment.get(name).cloned()
    })
}

pub(super) fn failed_labels(error: &TargetError) -> Vec<&str> {
    let TargetError::ChecksFailed { failures, .. } = error else {
        panic!("expected failed checks, got {error}");
    };
    failures
        .checks()
        .iter()
        .map(|check| check.label.as_str())
        .collect()
}

pub(super) fn coverage_report(workspace_covered: f64, rules_covered: f64) -> Value {
    json!({
        "schema": "rsproxy.coverage/v1",
        "source": "cargo-llvm-cov",
        "workspace": {
            "lines": 10_000,
            "covered": workspace_covered,
            "percent": workspace_covered / 100.0
        },
        "rules": {
            "lines": 10_000,
            "covered": rules_covered,
            "percent": rules_covered / 100.0
        },
        "production_files": 100
    })
}

pub(super) fn criterion_report(metric: &str, value: f64) -> Value {
    json!({
        "schema": "rsproxy.criterion/v1",
        "unit": "nanoseconds",
        "metrics": {
            (metric): {
                "mean_ns": value,
                "lower_ns": value,
                "upper_ns": value
            }
        }
    })
}

pub(super) fn e2e_report(rps: f64, p50: f64, p99: f64, rss: f64, whistle: f64) -> Value {
    json!({
        "schema": "rsproxy.e2e.performance/v1",
        "driver": "oha",
        "requests": 50_000,
        "concurrency": 32,
        "direct": {
            "requests_per_second": 120_000,
            "p50_us": 80,
            "p99_us": 250,
            "response_bytes": 51_200_000
        },
        "proxy": {
            "requests_per_second": rps,
            "p50_us": 250,
            "p99_us": 1_800,
            "response_bytes": 51_200_000
        },
        "added_latency": {"p50_us": p50, "p99_us": p99},
        "memory": {
            "empty_rss_kib": rss,
            "full_trace_rss_kib": 200_000,
            "growth_kib": 180_000
        },
        "whistle": {
            "requests_per_second": rps / whistle,
            "speedup": whistle
        }
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn soak_report(
    rps: f64,
    errors: f64,
    rss: f64,
    fd_end: f64,
    fd_peak: f64,
    pending: f64,
    queue_dropped: f64,
    rules: f64,
) -> Value {
    json!({
        "schema": "rsproxy.soak/v1",
        "driver": "oha",
        "duration": "90m",
        "warmup_duration": "30s",
        "started_at_epoch_seconds": 1,
        "elapsed_seconds": 5_400,
        "configured": {
            "qps": 1_000,
            "concurrency": 64,
            "rules": 1_001,
            "sample_interval_seconds": 60
        },
        "load": {
            "requests": 5_400_000,
            "requests_per_second": rps,
            "success_rate": if errors == 0.0 { 1.0 } else { 0.99 },
            "response_bytes": 5_400_000_u64 * 1_024,
            "status_200": 5_400_000.0 - errors,
            "errors": errors
        },
        "process": {
            "samples": 91,
            "rss_kib": {
                "start": 20_000,
                "end": 20_000.0 + rss,
                "max": 20_000.0 + rss,
                "end_growth": rss,
                "peak_growth": rss,
                "slope_kib_per_hour": 0,
                "last_half_slope_kib_per_hour": 0
            },
            "fds": {
                "start": 20,
                "end": 20.0 + fd_end,
                "max": 20.0 + fd_peak,
                "end_growth": fd_end,
                "peak_growth": fd_peak
            }
        },
        "rules": {"loaded": rules},
        "trace": {
            "sessions": 4_096,
            "max_sessions": 4_096,
            "queue_dropped": queue_dropped,
            "queue_memory_dropped": 0,
            "queue_bytes": 0,
            "pending_sessions": pending,
            "incomplete_sessions": 0,
            "orphan_events": 0,
            "total_memory_bytes": 1_000_000,
            "memory_budget_bytes": 67_108_864,
            "spill_errors": 0
        }
    })
}
