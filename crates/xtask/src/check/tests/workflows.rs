use super::super::workflows::{stable_action_for_test, violations_for_test};
use super::fixture::Fixture;

#[test]
fn workflow_check_accepts_final_inventory_and_commands() {
    let fixture = Fixture::new();
    fixture.workflows();
    assert!(
        violations_for_test(fixture.root())
            .expect("check workflow fixture")
            .is_empty()
    );
    assert!(stable_action_for_test("actions/checkout@v6"));
    assert!(stable_action_for_test("dtolnay/rust-toolchain@stable"));
    assert!(!stable_action_for_test("actions/checkout@main"));
    assert!(!stable_action_for_test("actions/checkout@develop"));
    assert!(!stable_action_for_test("actions/checkout"));
}

#[test]
fn workflow_check_rejects_top_level_write_permissions_at_any_indent() {
    for indent in ["  ", "    "] {
        let fixture = Fixture::new();
        fixture.workflows();
        fixture.write(
            ".github/workflows/release.yml",
            &format!(
                "name: Release\npermissions:\n{indent}contents: write\njobs:\n  test:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo xtask release \"$version\" --check\n"
            ),
        );

        let violations = violations_for_test(fixture.root()).expect("check top-level write");
        assert!(violations.iter().any(|violation| {
            violation.message.contains("workflow-level permission")
                && violation.message.contains("contents: write")
        }));
    }
}

#[test]
fn workflow_check_rejects_extra_files_old_commands_and_floating_actions() {
    let fixture = Fixture::new();
    fixture.workflows();
    fixture.write(".github/workflows/extra.yaml", "name: extra\n");
    fixture.write(
        ".github/workflows/ci.yml",
        "name: CI\njobs:\n  bad:\n    steps:\n      - uses: actions/checkout@main\n      - run: ./scripts/check.sh all\n",
    );

    let violations = violations_for_test(fixture.root()).expect("check invalid workflows");
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("inventory"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("check.sh"))
    );
    assert!(
        violations
            .iter()
            .any(|violation| violation.message.contains("not stable"))
    );
}
