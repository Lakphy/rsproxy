#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
OUTPUT=${RSPROXY_COVERAGE_OUTPUT:-"$ROOT/target/coverage/report.json"}
RAW_OUTPUT=${RSPROXY_COVERAGE_RAW_OUTPUT:-"$ROOT/target/coverage/llvm-summary.json"}
NO_RUN=${RSPROXY_COVERAGE_NO_RUN:-0}
ENFORCE=${RSPROXY_COVERAGE_ENFORCE:-1}
IGNORE='(/tests(/|\.rs$)|/benches/|/examples/)'

command -v jq >/dev/null 2>&1 || {
    echo "coverage collection requires jq" >&2
    exit 1
}
cargo llvm-cov --version >/dev/null 2>&1 || {
    echo "coverage collection requires cargo-llvm-cov" >&2
    exit 1
}

mkdir -p "$(dirname "$OUTPUT")" "$(dirname "$RAW_OUTPUT")"
cd "$ROOT"
if [ "$NO_RUN" = "1" ]; then
    cargo llvm-cov report \
        --ignore-filename-regex "$IGNORE" \
        --json --summary-only --output-path "$RAW_OUTPUT"
else
    cargo llvm-cov --workspace --all-targets --no-fail-fast --locked \
        --ignore-filename-regex "$IGNORE" \
        --json --summary-only --output-path "$RAW_OUTPUT"
fi

jq '
    def metric($lines; $covered): {
        lines: $lines,
        covered: $covered,
        percent: (if $lines == 0 then 0 else ($covered * 100 / $lines) end)
    };
    .data[0] as $coverage |
    [$coverage.files[] |
        select(.filename | contains("/crates/rsproxy-rules/src/")) |
        .summary.lines] as $rules |
    {
        schema: "rsproxy.coverage/v1",
        source: "cargo-llvm-cov",
        workspace: metric(
            $coverage.totals.lines.count;
            $coverage.totals.lines.covered
        ),
        rules: metric(
            ($rules | map(.count) | add);
            ($rules | map(.covered) | add)
        ),
        production_files: ($coverage.files | length)
    }
' "$RAW_OUTPUT" >"$OUTPUT"

cat "$OUTPUT"
if [ "$ENFORCE" = "1" ]; then
    cargo xtask targets coverage "$OUTPUT"
fi
