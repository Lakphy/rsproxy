use serde_json::Value;

use super::super::{TargetCommand, TargetError};
use super::support::{ReportFile, failed_labels, run, soak_report};

#[test]
fn soak_contract_covers_correctness_resource_trace_and_rule_targets() {
    let passing = ReportFile::new(&soak_report(
        950.0, 0.0, 32_768.0, 16.0, 144.0, 0.0, 0.0, 1_001.0,
    ));
    let outcome = run(
        TargetCommand::Soak {
            report: passing.path().to_path_buf(),
        },
        &[],
    )
    .expect("soak boundary report passes");
    assert_eq!(outcome.checks().len(), 14);

    let base = soak_report(950.0, 0.0, 32_768.0, 16.0, 144.0, 0.0, 0.0, 1_001.0);
    let failures = [
        (
            "elapsed-time",
            mutate(&base, |report| report["elapsed_seconds"] = 5_399.into()),
        ),
        (
            "request-volume",
            mutate(&base, |report| {
                report["load"]["requests"] = 4_999_999.into();
                report["load"]["response_bytes"] = (4_999_999_u64 * 1_024).into();
                report["load"]["status_200"] = 4_999_999.into();
            }),
        ),
        (
            "sample-depth",
            mutate(&base, |report| report["process"]["samples"] = 89.into()),
        ),
        (
            "rss-steady-slope",
            mutate(&base, |report| {
                report["process"]["rss_kib"]["last_half_slope_kib_per_hour"] = 1_025.into();
            }),
        ),
        (
            "request-rate",
            soak_report(899.0, 0.0, 32_768.0, 16.0, 144.0, 0.0, 0.0, 1_001.0),
        ),
        (
            "load-correctness",
            soak_report(950.0, 1.0, 32_768.0, 16.0, 144.0, 0.0, 0.0, 1_001.0),
        ),
        (
            "rss-peak-growth",
            soak_report(950.0, 0.0, 32_769.0, 16.0, 144.0, 0.0, 0.0, 1_001.0),
        ),
        (
            "fd-end-growth",
            soak_report(950.0, 0.0, 32_768.0, 17.0, 144.0, 0.0, 0.0, 1_001.0),
        ),
        (
            "fd-peak-growth",
            soak_report(950.0, 0.0, 32_768.0, 16.0, 145.0, 0.0, 0.0, 1_001.0),
        ),
        (
            "trace-drained",
            soak_report(950.0, 0.0, 32_768.0, 16.0, 144.0, 1.0, 0.0, 1_001.0),
        ),
        (
            "trace-loss",
            soak_report(950.0, 0.0, 32_768.0, 16.0, 144.0, 0.0, 1.0, 1_001.0),
        ),
        (
            "rules-loaded",
            soak_report(950.0, 0.0, 32_768.0, 16.0, 144.0, 0.0, 0.0, 1_000.0),
        ),
    ];

    for (expected_label, report) in failures {
        let report = ReportFile::new(&report);
        let error = run(
            TargetCommand::Soak {
                report: report.path().to_path_buf(),
            },
            &[],
        )
        .expect_err("contract failure must be rejected");
        let labels = failed_labels(&error);
        assert!(
            labels.contains(&expected_label),
            "expected {expected_label}, got {labels:?}"
        );
    }
}

#[test]
fn soak_honors_every_environment_override() {
    let mut report = soak_report(800.0, 0.0, 40_000.0, 20.0, 200.0, 0.0, 0.0, 1_001.0);
    report["elapsed_seconds"] = 100.into();
    report["load"]["requests"] = 100.into();
    report["load"]["status_200"] = 100.into();
    report["load"]["response_bytes"] = (100 * 1_024).into();
    report["process"]["samples"] = 2.into();
    report["process"]["rss_kib"]["last_half_slope_kib_per_hour"] = 2_000.into();
    let report = ReportFile::new(&report);
    run(
        TargetCommand::Soak {
            report: report.path().to_path_buf(),
        },
        &[
            ("RSPROXY_SOAK_MAX_RSS_GROWTH_KIB", "40000"),
            ("RSPROXY_SOAK_MAX_FD_END_GROWTH", "20"),
            ("RSPROXY_SOAK_MAX_FD_PEAK_GROWTH", "200"),
            ("RSPROXY_SOAK_MIN_RATE_RATIO", "0.8"),
            ("RSPROXY_SOAK_MIN_ELAPSED_SECONDS", "100"),
            ("RSPROXY_SOAK_MIN_REQUESTS", "100"),
            ("RSPROXY_SOAK_MIN_SAMPLES", "2"),
            ("RSPROXY_SOAK_MAX_RSS_LAST_HALF_SLOPE_KIB_PER_HOUR", "2000"),
        ],
    )
    .expect("all soak environment overrides are honored");
}

#[test]
fn fd_threshold_precedence_and_derived_peak_match_shell_contract() {
    let end = ReportFile::new(&soak_report(
        950.0, 0.0, 32_768.0, 17.0, 144.0, 0.0, 0.0, 1_001.0,
    ));
    run(
        TargetCommand::Soak {
            report: end.path().to_path_buf(),
        },
        &[("RSPROXY_SOAK_MAX_FD_GROWTH", "17")],
    )
    .expect("legacy FD growth variable remains a fallback");
    let error = run(
        TargetCommand::Soak {
            report: end.path().to_path_buf(),
        },
        &[
            ("RSPROXY_SOAK_MAX_FD_GROWTH", "17"),
            ("RSPROXY_SOAK_MAX_FD_END_GROWTH", "16"),
        ],
    )
    .expect_err("new FD end variable takes precedence");
    assert_eq!(failed_labels(&error), ["fd-end-growth"]);

    let peak = ReportFile::new(&soak_report(
        950.0, 0.0, 32_768.0, 16.0, 145.0, 0.0, 0.0, 1_001.0,
    ));
    run(
        TargetCommand::Soak {
            report: peak.path().to_path_buf(),
        },
        &[("RSPROXY_SOAK_FD_PEAK_HEADROOM", "17")],
    )
    .expect("peak limit is twice concurrency plus headroom");
}

#[test]
fn fd_environment_values_must_be_non_negative_integers() {
    let report = ReportFile::new(&soak_report(
        950.0, 0.0, 32_768.0, 16.0, 144.0, 0.0, 0.0, 1_001.0,
    ));
    for (name, value) in [
        ("RSPROXY_SOAK_MAX_FD_END_GROWTH", "-1"),
        ("RSPROXY_SOAK_FD_PEAK_HEADROOM", "1.5"),
        ("RSPROXY_SOAK_MAX_FD_PEAK_GROWTH", "many"),
    ] {
        let error = run(
            TargetCommand::Soak {
                report: report.path().to_path_buf(),
            },
            &[(name, value)],
        )
        .expect_err("invalid FD threshold fails");
        assert!(matches!(error, TargetError::InvalidEnvironment { .. }));
        assert!(error.to_string().contains(name));
    }
}

#[test]
fn soak_schema_relations_include_precise_field_context() {
    let mut report = soak_report(950.0, 0.0, 32_768.0, 16.0, 144.0, 0.0, 0.0, 1_001.0);
    report["process"]["fds"]["peak_growth"] = 15.into();
    let report = ReportFile::new(&report);
    let error = run(
        TargetCommand::Soak {
            report: report.path().to_path_buf(),
        },
        &[],
    )
    .expect_err("peak growth cannot be below end growth");
    assert!(error.to_string().contains("process.fds.peak_growth"));
}

fn mutate(source: &Value, change: impl FnOnce(&mut Value)) -> Value {
    let mut report = source.clone();
    change(&mut report);
    report
}
