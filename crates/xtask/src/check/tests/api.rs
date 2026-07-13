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
