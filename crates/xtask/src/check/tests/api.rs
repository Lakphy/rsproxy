use std::fs;
use std::path::Path;

use super::super::api::reconcile_snapshot_for_test;
use super::super::{CheckError, CheckKind, validate_options};
use super::fixture::Fixture;

const RELATIVE_SNAPSHOT: &str = "crates/example/api.txt";

#[test]
fn equal_snapshot_accepts_line_endings_and_a_missing_final_newline() {
    let fixture = Fixture::new();
    fixture.write(
        RELATIVE_SNAPSHOT,
        "pub mod example\npub fn example::value()\n",
    );
    let mut violations = Vec::new();

    reconcile(
        &fixture,
        "pub mod example\r\npub fn example::value()",
        false,
        &mut violations,
    )
    .expect("compare equal API snapshot");

    assert!(violations.is_empty());
}

#[test]
fn drift_reports_the_first_changed_line_and_bless_command() {
    let fixture = Fixture::new();
    fixture.write(
        RELATIVE_SNAPSHOT,
        "pub mod example\npub fn example::old()\n",
    );
    let mut violations = Vec::new();

    reconcile(
        &fixture,
        "pub mod example\npub fn example::new()\n",
        false,
        &mut violations,
    )
    .expect("compare drifted API snapshot");

    assert_eq!(violations.len(), 1);
    let message = &violations[0].message;
    assert!(message.contains("line 2"));
    assert!(message.contains("example::old"));
    assert!(message.contains("example::new"));
    assert!(message.contains("cargo xtask check api --bless"));
}

#[test]
fn missing_snapshot_reports_the_creation_command_without_writing() {
    let fixture = Fixture::new();
    let mut violations = Vec::new();

    reconcile(&fixture, "pub mod example\n", false, &mut violations)
        .expect("compare missing API snapshot");

    assert_eq!(violations.len(), 1);
    assert!(violations[0].message.contains("snapshot is missing"));
    assert!(
        violations[0]
            .message
            .contains("cargo xtask check api --bless")
    );
    assert!(!fixture.root().join(RELATIVE_SNAPSHOT).exists());
}

#[test]
fn bless_replaces_snapshot_with_canonical_generated_output() {
    let fixture = Fixture::new();
    fixture.write(RELATIVE_SNAPSHOT, "stale\n");
    let mut violations = Vec::new();

    reconcile(
        &fixture,
        "pub mod example\r\npub fn example::value()",
        true,
        &mut violations,
    )
    .expect("bless API snapshot");

    assert!(violations.is_empty());
    assert_eq!(
        fs::read_to_string(fixture.root().join(RELATIVE_SNAPSHOT))
            .expect("read blessed API snapshot"),
        "pub mod example\npub fn example::value()\n"
    );
}

#[test]
fn bless_is_limited_to_api_and_all_checks() {
    assert!(validate_options(CheckKind::Api, true).is_ok());
    assert!(validate_options(CheckKind::All, true).is_ok());
    assert!(validate_options(CheckKind::Lines, false).is_ok());

    let error = validate_options(CheckKind::Lines, true).expect_err("reject unrelated bless");
    assert!(matches!(
        &error,
        CheckError::InvalidBlessKind {
            kind: CheckKind::Lines
        }
    ));
    assert!(error.to_string().contains("check api"));
    assert!(error.to_string().contains("check all"));
}

#[test]
fn drift_rendering_handles_ended_and_long_lines() {
    let fixture = Fixture::new();
    fixture.write(RELATIVE_SNAPSHOT, "first\nsecond\n");
    let mut violations = Vec::new();
    reconcile(&fixture, "first\n", false, &mut violations).unwrap();
    assert!(violations[0].message.contains("<end of file>"));

    let long_expected = "a".repeat(121);
    let long_generated = "b".repeat(121);
    fixture.write(RELATIVE_SNAPSHOT, &long_expected);
    violations.clear();
    reconcile(&fixture, &long_generated, false, &mut violations).unwrap();
    assert!(violations[0].message.contains('…'));
}

#[test]
fn snapshot_read_and_bless_write_errors_keep_the_path_and_action() {
    let fixture = Fixture::new();
    fixture.write(&format!("{RELATIVE_SNAPSHOT}/child"), "not a snapshot");
    let mut violations = Vec::new();
    let error = reconcile(&fixture, "generated\n", false, &mut violations)
        .expect_err("reading a directory as a snapshot must fail");
    assert!(matches!(error, CheckError::Io { .. }));
    assert!(error.to_string().contains("read public API snapshot"));

    let fixture = Fixture::new();
    let error = reconcile(&fixture, "generated\n", true, &mut Vec::new())
        .expect_err("blessing without a snapshot parent must fail");
    assert!(matches!(error, CheckError::Io { .. }));
    assert!(error.to_string().contains("write public API snapshot"));
}

fn reconcile(
    fixture: &Fixture,
    generated: &str,
    bless: bool,
    violations: &mut Vec<super::super::Violation>,
) -> Result<(), CheckError> {
    reconcile_snapshot_for_test(
        &fixture.root().join(RELATIVE_SNAPSHOT),
        Path::new(RELATIVE_SNAPSHOT),
        generated,
        bless,
        violations,
    )
}
