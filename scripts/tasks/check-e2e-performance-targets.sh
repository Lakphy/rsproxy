#!/bin/sh
set -eu

REPORT=${1:?usage: check-e2e-performance-targets.sh REPORT}
MIN_RPS=${RSPROXY_PERF_MIN_RPS:-80000}
MAX_P50_US=${RSPROXY_PERF_MAX_ADDED_P50_US:-300}
MAX_P99_US=${RSPROXY_PERF_MAX_ADDED_P99_US:-2000}
MAX_EMPTY_RSS_KIB=${RSPROXY_PERF_MAX_EMPTY_RSS_KIB:-30720}
REQUIRE_WHISTLE=${RSPROXY_PERF_REQUIRE_WHISTLE:-0}
failed=0

command -v jq >/dev/null 2>&1 || {
    echo "performance target check requires jq" >&2
    exit 1
}

jq -e '
    .schema == "rsproxy.e2e.performance/v1" and
    .driver == "oha" and
    (.requests | type == "number" and . > 0) and
    (.concurrency | type == "number" and . > 0) and
    all(.direct, .proxy;
        . as $metric |
        ($metric.requests_per_second | type == "number") and
        $metric.requests_per_second > 0 and
        ($metric.p50_us | type == "number") and
        $metric.p50_us >= 0 and
        ($metric.p99_us | type == "number") and
        $metric.p99_us >= $metric.p50_us and
        ($metric.response_bytes | type == "number") and
        $metric.response_bytes > 0) and
    (.added_latency.p50_us | type == "number" and . >= 0) and
    (.added_latency.p99_us | type == "number" and . >= 0) and
    (.memory.empty_rss_kib | type == "number" and . > 0)
' "$REPORT" >/dev/null || {
    echo "invalid e2e performance report: $REPORT" >&2
    exit 1
}

check() {
    label=$1
    query=$2
    observed=$3
    target=$4
    if jq -e "$query" "$REPORT" >/dev/null; then
        printf 'PASS %-24s observed=%s target=%s\n' "$label" "$observed" "$target"
    else
        printf 'FAIL %-24s observed=%s target=%s\n' \
            "$label" "$observed" "$target" >&2
        failed=1
    fi
}

proxy_rps=$(jq -r '.proxy.requests_per_second' "$REPORT")
added_p50=$(jq -r '.added_latency.p50_us' "$REPORT")
added_p99=$(jq -r '.added_latency.p99_us' "$REPORT")
empty_rss=$(jq -r '.memory.empty_rss_kib' "$REPORT")

check throughput \
    ".proxy.requests_per_second >= $MIN_RPS" \
    "$proxy_rps rps" ">= $MIN_RPS rps"
check added-latency-p50 \
    ".added_latency.p50_us < $MAX_P50_US" \
    "$added_p50 us" "< $MAX_P50_US us"
check added-latency-p99 \
    ".added_latency.p99_us < $MAX_P99_US" \
    "$added_p99 us" "< $MAX_P99_US us"
check empty-rss \
    ".memory.empty_rss_kib < $MAX_EMPTY_RSS_KIB" \
    "$empty_rss KiB" "< $MAX_EMPTY_RSS_KIB KiB"

if [ "$REQUIRE_WHISTLE" = "1" ]; then
    whistle_speedup=$(jq -r '.whistle.speedup // "missing"' "$REPORT")
    check whistle-speedup \
        '.whistle != null and .whistle.speedup >= 10' \
        "$whistle_speedup x" ">= 10x"
fi

[ "$failed" -eq 0 ] || exit 1
echo "All enabled e2e performance targets passed."
