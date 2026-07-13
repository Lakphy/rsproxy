#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
ITERATIONS=${RSPROXY_RULES_BENCH_ITERATIONS:-50000}
WARMUP=${RSPROXY_RULES_BENCH_WARMUP:-1000}
MAX_P99_NS=${RSPROXY_RULES_BENCH_MAX_P99_NS:-10000}
SKIP_BUILD=${RSPROXY_RULES_BENCH_SKIP_BUILD:-0}
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-rules-bench.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

command -v jq >/dev/null 2>&1 || {
    echo "rules acceptance benchmark requires jq" >&2
    exit 1
}
if [ "$SKIP_BUILD" != "1" ]; then
    cargo build --release -p rsproxy-cli --bin rsproxy --locked --manifest-path "$ROOT/Cargo.toml"
fi

awk 'BEGIN {
    for (i = 0; i < 10000; i++) {
        if (i % 5 == 0) {
            printf "/^http:\\/\\/bench-%d\\.example\\.test\\/api\\/[0-9]+$/ status(200)\n", i
        } else {
            printf "bench-%d.example.test/api status(200) when method(GET)\n", i
        }
    }
}' >"$TMP_ROOT/mixed.rules"

RESULT=$(
    "$ROOT/target/release/rsproxy" rules bench \
        --file "$TMP_ROOT/mixed.rules" \
        --url http://bench-9995.example.test/api/42 \
        --iterations "$ITERATIONS" \
        --warmup "$WARMUP" \
        --json
)

printf '%s\n' "$RESULT" | jq -e \
    --argjson iterations "$ITERATIONS" \
    --argjson max_p99_ns "$MAX_P99_NS" '
        .iterations == $iterations and
        .rules == 10000 and
        .indexed_rules == 8000 and
        .global_rules == 0 and
        .prefilter_rules == 2000 and
        .matched_actions == (.iterations + .warmup) and
        .p50_ns > 0 and
        .p99_ns > 0 and
        .p99_ns < $max_p99_ns
    ' >/dev/null || {
        echo "10k mixed-rule p99 acceptance failed (limit ${MAX_P99_NS}ns):" >&2
        printf '%s\n' "$RESULT" | jq '.' >&2
        exit 1
    }

printf '%s\n' "$RESULT"
