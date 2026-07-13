use super::super::{CheckError, CheckKind, run};
use super::fixture::Fixture;

#[test]
fn line_check_honors_limit_and_exclusions() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.write(
        "xtask.toml",
        "[lines]\nlimit = 2\nexclude = [\"fuzz/target\"]\n",
    );
    fixture.write("fuzz/target/generated.rs", "a\nb\nc\nd\n");

    let report = run(fixture.root(), CheckKind::Lines).expect("line check passes");
    assert_eq!(report.checks.len(), 1);
    assert_eq!(report.checks[0].kind, CheckKind::Lines);
}

#[test]
fn line_check_reports_every_oversized_rust_file() {
    let fixture = Fixture::new();
    fixture.basic_rust_tree();
    fixture.write(
        "xtask.toml",
        "[lines]\nlimit = 2\nexclude = [\"fuzz/target\"]\n",
    );
    fixture.write("crates/example/src/large.rs", "a\nb\nc\n");
    fixture.write("fuzz/fuzz_targets/large.rs", "a\nb\nc\n");

    let error = run(fixture.root(), CheckKind::Lines).expect_err("line check must fail");
    let CheckError::Violations(failures) = error else {
        panic!("unexpected line error: {error}");
    };
    assert_eq!(failures.violations.len(), 2);
}
