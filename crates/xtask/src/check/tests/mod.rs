mod fixture;
mod layout;
mod lines;
mod typed_errors;
mod whistle;
mod workflows;

use super::CheckKind;
use super::run;
use fixture::Fixture;

#[test]
fn all_runs_every_check_in_the_required_order() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.whistle();
    fixture.workflows();
    fixture.write(
        "xtask.toml",
        "[lines]\nlimit = 500\nexclude = [\"fuzz/target\"]\n",
    );
    let report = run(fixture.root(), CheckKind::All).expect("run all checks");
    assert_eq!(
        report
            .checks
            .iter()
            .map(|check| check.kind)
            .collect::<Vec<_>>(),
        vec![
            CheckKind::Lines,
            CheckKind::Layout,
            CheckKind::TypedErrors,
            CheckKind::Workflows,
        ]
    );
}
