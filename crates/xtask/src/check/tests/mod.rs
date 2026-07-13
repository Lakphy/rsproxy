mod api;
mod fixture;
mod layout;
mod lines;
mod typed_errors;
mod whistle;
mod workflows;

use super::{CheckFailures, CheckKind, Violation, expanded_checks, run};

#[test]
fn all_expands_every_check_in_the_required_order() {
    assert_eq!(
        expanded_checks(CheckKind::All),
        &[
            CheckKind::Api,
            CheckKind::Lines,
            CheckKind::Layout,
            CheckKind::TypedErrors,
            CheckKind::Workflows,
        ],
    );
}

#[test]
fn check_kinds_and_failures_have_actionable_rendering() {
    for (kind, expected) in [
        (CheckKind::Api, "api"),
        (CheckKind::Lines, "lines"),
        (CheckKind::Layout, "layout"),
        (CheckKind::TypedErrors, "typed-errors"),
        (CheckKind::Workflows, "workflows"),
        (CheckKind::All, "all"),
    ] {
        assert_eq!(kind.to_string(), expected);
    }

    let failures = CheckFailures {
        kind: CheckKind::Layout,
        violations: vec![
            Violation::new("first.rs", "first failure"),
            Violation::new("second.rs", "second failure"),
        ],
    };
    assert_eq!(
        failures.to_string(),
        "layout check failed:\n  first.rs: first failure\n  second.rs: second failure\n"
    );
}

#[test]
fn public_runner_dispatches_typed_error_and_workflow_checks() {
    let typed = fixture::Fixture::new();
    typed.basic_rust_tree();
    assert_eq!(
        run(typed.root(), CheckKind::TypedErrors)
            .unwrap()
            .checks
            .len(),
        1
    );

    let workflows = fixture::Fixture::new();
    workflows.workflows();
    assert_eq!(
        run(workflows.root(), CheckKind::Workflows)
            .unwrap()
            .checks
            .len(),
        1
    );
}
