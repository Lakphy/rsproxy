use std::path::PathBuf;

use super::super::layout::{dedicated_test_path_for_test, rust_violations_for_test};
use super::fixture::Fixture;

#[test]
fn layout_accepts_dedicated_unit_and_integration_tests() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.write(
        "crates/example/src/tests.rs",
        "#[test]\nfn unit_test() {}\n",
    );
    fixture.write(
        "crates/example/tests/behavior.rs",
        "#[tokio::test]\nasync fn integration_test() {}\n",
    );

    assert!(
        rust_violations_for_test(fixture.root())
            .expect("check Rust layout")
            .is_empty()
    );
    assert!(dedicated_test_path_for_test(&PathBuf::from(
        "crates/example/src/module/tests/case.rs"
    )));
}

#[test]
fn layout_rejects_inline_misplaced_and_missing_integration_tests() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.write(
        "crates/example/src/lib.rs",
        "#[cfg(test)] mod tests { #[test] fn inline() {} }\n",
    );
    fixture.write(
        "crates/example/src/helper.rs",
        "#[test]\nfn misplaced() {}\n",
    );
    fixture.remove("crates/example/tests");

    let violations = rust_violations_for_test(fixture.root()).expect("check invalid layout");
    assert_eq!(violations.len(), 4);
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("inline"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("missing"))
    );
}

#[test]
fn layout_rejects_empty_crate_directories_with_a_fix_hint() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.write("crates/example/src/obsolete/.keep", "");
    fixture.remove("crates/example/src/obsolete/.keep");

    let violations = rust_violations_for_test(fixture.root()).expect("check empty directory");
    assert_eq!(violations.len(), 1);
    assert_eq!(
        violations[0].path,
        PathBuf::from("crates/example/src/obsolete")
    );
    assert!(violations[0].message.contains("remove it"));
}
