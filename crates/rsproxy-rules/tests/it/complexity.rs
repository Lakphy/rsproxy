use crate::fuzz_harness;
use std::time::{Duration, Instant};

const MAX_INPUT: usize = 64 * 1024;
#[cfg(not(coverage))]
// `cargo nextest --all-targets` runs this safety test alongside Criterion
// targets on CI. Keep a generous finite ceiling so scheduler contention does
// not masquerade as an algorithmic regression; dedicated benchmarks own the
// tighter performance contract.
const CASE_BUDGET: Duration = Duration::from_secs(10);
// LLVM source-coverage counters deliberately instrument every branch. Keep the
// same finite-complexity assertion without confusing instrumentation overhead
// with the normal-build performance contract above.
#[cfg(coverage)]
const CASE_BUDGET: Duration = Duration::from_secs(15);

#[test]
fn max_size_parse_resolve_inputs_stay_within_a_finite_complexity_budget() {
    let cases = [
        ("large-inline", large_inline()),
        ("many-rules", many_rules()),
        ("malformed-delimiters", malformed_delimiters()),
        ("fancy-backtrack", fancy_backtrack()),
        ("long-glob", long_glob()),
    ];

    for (name, input) in cases {
        assert!(input.len() <= MAX_INPUT, "{name} exceeds fuzz input limit");
        let started = Instant::now();
        fuzz_harness::exercise(input.as_bytes());
        let elapsed = started.elapsed();
        assert!(
            elapsed < CASE_BUDGET,
            "{name} took {elapsed:?}, budget is {CASE_BUDGET:?}"
        );
    }
}

#[test]
fn inline_parse_resolve_growth_is_bounded_across_an_eight_x_input_increase() {
    let small = minimum_elapsed(&large_inline_at(8 * 1024));
    let large = minimum_elapsed(&large_inline_at(64 * 1024));
    let allowed = small.saturating_mul(32) + Duration::from_millis(100);
    assert!(
        large <= allowed,
        "8x input growth took {large:?}; small={small:?}, allowed={allowed:?}"
    );
}

fn minimum_elapsed(input: &str) -> Duration {
    (0..3)
        .map(|_| {
            let started = Instant::now();
            fuzz_harness::exercise(input.as_bytes());
            started.elapsed()
        })
        .min()
        .unwrap()
}

fn large_inline() -> String {
    large_inline_at(MAX_INPUT)
}

fn large_inline_at(limit: usize) -> String {
    let prefix = r#"example.test req.header(x-large: ""#;
    let suffix = "\")";
    let payload = "a".repeat(limit.saturating_sub(prefix.len() + suffix.len()));
    format!("{prefix}{payload}{suffix}")
}

fn many_rules() -> String {
    let mut source = String::new();
    for index in 0..1_000 {
        let line = format!(
            "host{index}.example.test/api/{index} req.header(x-index: {index}) when method(GET)\n"
        );
        if source.len() + line.len() > MAX_INPUT {
            break;
        }
        source.push_str(&line);
    }
    source
}

fn malformed_delimiters() -> String {
    let prefix = "example.test req.header(x-malformed: ";
    format!("{prefix}{}", "(${".repeat((MAX_INPUT - prefix.len()) / 3))
}

fn fancy_backtrack() -> String {
    let source = r"/(a|aa)+(?=b)/ status(200)";
    let separator = "\n---request-url---\n";
    let url_prefix = "http://example.test/";
    let tail = "a".repeat(MAX_INPUT - source.len() - separator.len() - url_prefix.len());
    format!("{source}{separator}{url_prefix}{tail}")
}

fn long_glob() -> String {
    let repeated = "a".repeat(24 * 1024);
    let source = format!("example.test/{repeated}*tail status(204)");
    let separator = "\n---request-url---\n";
    let request = format!("http://example.test/{repeated}captured-tail");
    format!("{source}{separator}{request}")
}
