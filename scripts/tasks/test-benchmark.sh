#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
REQUESTS=${RSPROXY_BENCH_TEST_REQUESTS:-128}
CONCURRENCY=${RSPROXY_BENCH_TEST_CONCURRENCY:-8}
RESULT=$(RSPROXY_BENCH_REQUESTS=$REQUESTS \
    RSPROXY_BENCH_CONCURRENCY=$CONCURRENCY \
    "$ROOT/benches/e2e/benchmark.sh")

printf '%s\n' "$RESULT" | jq -e \
    --argjson requests "$REQUESTS" \
    --argjson concurrency "$CONCURRENCY" '
    .schema == "rsproxy-benchmark/v1" and
    .driver == "rsproxy-rust-h1" and
    .requests == $requests and
    .completed_requests == $requests and
    .concurrency == $concurrency and
    .response_bytes == ($requests * 1024) and
    .status_errors == 0 and
    .io_errors == 0 and
    .requests_per_second > 0 and
    .p99_us >= .p50_us
' >/dev/null

printf '%s\n' "$RESULT"
