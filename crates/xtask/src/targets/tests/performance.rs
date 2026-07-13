use super::super::{TargetCommand, TargetError};
use super::support::{ReportFile, criterion_report, e2e_report, failed_labels, run};

const TLS_METRIC: &str = "mitm_certificate/cached_tls_handshake";

#[test]
fn regression_contract_accepts_ten_percent_and_rejects_slowdown_or_missing_metric() {
    let baseline = ReportFile::new(&criterion_report("sample", 100.0));
    let within = ReportFile::new(&criterion_report("sample", 110.0));
    let regressed = ReportFile::new(&criterion_report("sample", 111.0));
    let missing = ReportFile::new(&criterion_report("other", 1.0));

    run(
        TargetCommand::Regression {
            baseline: baseline.path().to_path_buf(),
            current: within.path().to_path_buf(),
            tolerance_percent: 10.0,
        },
        &[],
    )
    .expect("exactly ten percent is allowed");

    let error = run(
        TargetCommand::Regression {
            baseline: baseline.path().to_path_buf(),
            current: regressed.path().to_path_buf(),
            tolerance_percent: 10.0,
        },
        &[],
    )
    .expect_err("eleven percent is a regression");
    let TargetError::Regressions { failures, .. } = error else {
        panic!("unexpected error: {error}");
    };
    assert_eq!(failures.failures().len(), 1);
    assert_eq!(failures.failures()[0].metric, "sample");

    let error = run(
        TargetCommand::Regression {
            baseline: baseline.path().to_path_buf(),
            current: missing.path().to_path_buf(),
            tolerance_percent: 10.0,
        },
        &[],
    )
    .expect_err("current report must include every baseline metric");
    assert!(matches!(error, TargetError::MissingMetrics { .. }));
    assert!(error.to_string().contains("sample"));
}

#[test]
fn e2e_contract_covers_every_default_threshold() {
    let passing = ReportFile::new(&e2e_report(80_000.0, 299.0, 1_999.0, 30_719.0, 10.0));
    run(
        TargetCommand::E2e {
            report: passing.path().to_path_buf(),
        },
        &[],
    )
    .expect("e2e boundary report passes without Whistle requirement");
    run(
        TargetCommand::E2e {
            report: passing.path().to_path_buf(),
        },
        &[("RSPROXY_PERF_REQUIRE_WHISTLE", "1")],
    )
    .expect("ten-times Whistle speedup passes");

    for (label, report) in [
        (
            "throughput",
            e2e_report(79_999.0, 299.0, 1_999.0, 30_719.0, 10.0),
        ),
        (
            "added-latency-p50",
            e2e_report(80_000.0, 300.0, 1_999.0, 30_719.0, 10.0),
        ),
        (
            "added-latency-p99",
            e2e_report(80_000.0, 299.0, 2_000.0, 30_719.0, 10.0),
        ),
        (
            "empty-rss",
            e2e_report(80_000.0, 299.0, 1_999.0, 30_720.0, 10.0),
        ),
    ] {
        let report = ReportFile::new(&report);
        let error = run(
            TargetCommand::E2e {
                report: report.path().to_path_buf(),
            },
            &[],
        )
        .expect_err("threshold equality or below-minimum must fail");
        assert_eq!(failed_labels(&error), [label]);
    }

    let whistle = ReportFile::new(&e2e_report(80_000.0, 299.0, 1_999.0, 30_719.0, 9.99));
    let error = run(
        TargetCommand::E2e {
            report: whistle.path().to_path_buf(),
        },
        &[("RSPROXY_PERF_REQUIRE_WHISTLE", "1")],
    )
    .expect_err("Whistle speedup below ten times fails when required");
    assert_eq!(failed_labels(&error), ["whistle-speedup"]);
}

#[test]
fn criterion_absolute_target_uses_upper_bound_exclusively() {
    let mut passing = criterion_report(TLS_METRIC, 2_999_000.0);
    passing["metrics"][TLS_METRIC]["lower_ns"] = 2_997_999.into();
    passing["metrics"][TLS_METRIC]["upper_ns"] = 2_999_999.into();
    let passing = ReportFile::new(&passing);
    run(
        TargetCommand::Criterion {
            report: passing.path().to_path_buf(),
        },
        &[],
    )
    .expect("upper confidence bound below three milliseconds passes");

    let mut failing = criterion_report(TLS_METRIC, 2_999_000.0);
    failing["metrics"][TLS_METRIC]["lower_ns"] = 2_998_000.into();
    failing["metrics"][TLS_METRIC]["upper_ns"] = 3_000_000.into();
    let failing = ReportFile::new(&failing);
    let error = run(
        TargetCommand::Criterion {
            report: failing.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("three-millisecond upper confidence bound fails");
    assert_eq!(failed_labels(&error), ["cached-tls-handshake"]);
}

#[test]
fn performance_thresholds_honor_all_environment_overrides() {
    let report = ReportFile::new(&e2e_report(70_000.0, 350.0, 2_500.0, 35_000.0, 1.0));
    run(
        TargetCommand::E2e {
            report: report.path().to_path_buf(),
        },
        &[
            ("RSPROXY_PERF_MIN_RPS", "70000"),
            ("RSPROXY_PERF_MAX_ADDED_P50_US", "351"),
            ("RSPROXY_PERF_MAX_ADDED_P99_US", "2501"),
            ("RSPROXY_PERF_MAX_EMPTY_RSS_KIB", "35001"),
            ("RSPROXY_PERF_REQUIRE_WHISTLE", "not-one"),
        ],
    )
    .expect("numeric performance overrides and exact whistle flag are honored");

    let target = ReportFile::new(&criterion_report(TLS_METRIC, 3_000_000.0));
    run(
        TargetCommand::Criterion {
            report: target.path().to_path_buf(),
        },
        &[("RSPROXY_PERF_MAX_TLS_HANDSHAKE_NS", "3000001")],
    )
    .expect("Criterion maximum override is honored");
}

#[test]
fn criterion_schema_errors_identify_the_metric_field() {
    let mut report = criterion_report(TLS_METRIC, 10.0);
    report["metrics"][TLS_METRIC]["lower_ns"] = 11.into();
    let report = ReportFile::new(&report);
    let error = run(
        TargetCommand::Criterion {
            report: report.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("lower confidence bound cannot exceed mean");
    assert!(error.to_string().contains(TLS_METRIC));
    assert!(error.to_string().contains("lower_ns"));
}
