use std::fs;

use super::super::whistle::violations_for_test;
use super::fixture::Fixture;

#[test]
fn whistle_check_accepts_pinned_fixture_hashes_and_driver() {
    let fixture = Fixture::new();
    fixture.whistle();
    assert!(
        violations_for_test(fixture.root())
            .expect("check Whistle fixture")
            .is_empty()
    );
}

#[test]
fn whistle_check_reports_checkout_hash_and_driver_drift() {
    let fixture = Fixture::new();
    fixture.whistle();
    fs::create_dir(fixture.root().join("whistle")).expect("create forbidden checkout");
    fixture.write(
        "crates/rsproxy-rules/tests/fixtures/whistle-2.10.5/docs/evidence-00.txt",
        "drifted\n",
    );
    fixture.write(
        "benches/e2e/whistle-driver/package.json",
        "{\"dependencies\":{\"whistle\":\"latest\"}}\n",
    );

    let violations = violations_for_test(fixture.root()).expect("check drifted fixture");
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("checkout"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("SHA-256"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("driver"))
    );
}
