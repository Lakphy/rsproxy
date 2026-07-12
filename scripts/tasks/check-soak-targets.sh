#!/bin/sh
set -eu

REPORT=${1:?usage: check-soak-targets.sh REPORT}
MAX_RSS_GROWTH_KIB=${RSPROXY_SOAK_MAX_RSS_GROWTH_KIB:-32768}
MAX_FD_END_GROWTH=${RSPROXY_SOAK_MAX_FD_END_GROWTH:-${RSPROXY_SOAK_MAX_FD_GROWTH:-16}}
FD_PEAK_HEADROOM=${RSPROXY_SOAK_FD_PEAK_HEADROOM:-16}
MIN_RATE_RATIO=${RSPROXY_SOAK_MIN_RATE_RATIO:-0.90}
MIN_ELAPSED_SECONDS=${RSPROXY_SOAK_MIN_ELAPSED_SECONDS:-5400}
MIN_REQUESTS=${RSPROXY_SOAK_MIN_REQUESTS:-5000000}
MIN_SAMPLES=${RSPROXY_SOAK_MIN_SAMPLES:-90}
MAX_RSS_LAST_HALF_SLOPE=${RSPROXY_SOAK_MAX_RSS_LAST_HALF_SLOPE_KIB_PER_HOUR:-1024}
failed=0

command -v jq >/dev/null 2>&1 || {
    echo "soak target check requires jq" >&2
    exit 1
}

jq -e '
    .schema == "rsproxy.soak/v1" and
    .driver == "oha" and
    (.duration | type == "string" and length > 0) and
    (.configured.qps | type == "number" and . > 0) and
    (.configured.concurrency | type == "number" and . > 0) and
    (.configured.rules | type == "number" and . > 0) and
    (.load.requests | type == "number" and . > 0) and
    (.load.requests_per_second | type == "number" and . > 0) and
    (.process.samples | type == "number" and . >= 2) and
    (.process.rss_kib.slope_kib_per_hour | type == "number") and
    (.process.rss_kib.last_half_slope_kib_per_hour | type == "number") and
    all(.process.rss_kib, .process.fds;
        . as $metric |
        ($metric.start | type == "number") and $metric.start >= 0 and
        ($metric.end | type == "number") and $metric.end >= 0 and
        ($metric.max | type == "number") and $metric.max >= $metric.start and
        ($metric.end_growth | type == "number") and $metric.end_growth >= 0 and
        ($metric.peak_growth | type == "number") and
            $metric.peak_growth >= $metric.end_growth) and
    (.trace | type == "object")
' "$REPORT" >/dev/null || {
    echo "invalid soak report: $REPORT" >&2
    exit 1
}

for value in "$MAX_FD_END_GROWTH" "$FD_PEAK_HEADROOM"; do
    case $value in
        ''|*[!0-9]*) echo "FD thresholds must be non-negative integers" >&2; exit 1 ;;
    esac
done
if [ -n "${RSPROXY_SOAK_MAX_FD_PEAK_GROWTH:-}" ]; then
    MAX_FD_PEAK_GROWTH=$RSPROXY_SOAK_MAX_FD_PEAK_GROWTH
    case $MAX_FD_PEAK_GROWTH in
        *[!0-9]*) echo "FD peak threshold must be a non-negative integer" >&2; exit 1 ;;
    esac
else
    MAX_FD_PEAK_GROWTH=$(jq -r --argjson headroom "$FD_PEAK_HEADROOM" \
        '.configured.concurrency * 2 + $headroom' "$REPORT")
fi

check() {
    label=$1
    query=$2
    observed=$3
    target=$4
    if jq -e "$query" "$REPORT" >/dev/null; then
        printf 'PASS %-22s observed=%s target=%s\n' "$label" "$observed" "$target"
    else
        printf 'FAIL %-22s observed=%s target=%s\n' \
            "$label" "$observed" "$target" >&2
        failed=1
    fi
}

rate=$(jq -r '.load.requests_per_second' "$REPORT")
elapsed=$(jq -r '.elapsed_seconds' "$REPORT")
requests=$(jq -r '.load.requests' "$REPORT")
samples=$(jq -r '.process.samples' "$REPORT")
rss_peak=$(jq -r '.process.rss_kib.peak_growth' "$REPORT")
rss_end=$(jq -r '.process.rss_kib.end_growth' "$REPORT")
rss_last_half_slope=$(jq -r '.process.rss_kib.last_half_slope_kib_per_hour' "$REPORT")
fd_peak=$(jq -r '.process.fds.peak_growth' "$REPORT")
fd_end=$(jq -r '.process.fds.end_growth' "$REPORT")

check request-rate \
    ".load.requests_per_second >= (.configured.qps * $MIN_RATE_RATIO)" \
    "$rate rps" ">= configured qps * $MIN_RATE_RATIO"
check elapsed-time \
    ".elapsed_seconds >= $MIN_ELAPSED_SECONDS" \
    "$elapsed seconds" ">= $MIN_ELAPSED_SECONDS seconds"
check request-volume \
    ".load.requests >= $MIN_REQUESTS" \
    "$requests" ">= $MIN_REQUESTS"
check sample-depth \
    ".process.samples >= $MIN_SAMPLES" \
    "$samples" ">= $MIN_SAMPLES"
check load-correctness \
    '.load.success_rate == 1 and .load.errors == 0 and .load.status_200 == .load.requests and .load.response_bytes == (.load.requests * 1024)' \
    "$(jq -r '.load.success_rate' "$REPORT") success" "exact 200/bytes and zero errors"
check rss-peak-growth \
    ".process.rss_kib.peak_growth <= $MAX_RSS_GROWTH_KIB" \
    "$rss_peak KiB" "<= $MAX_RSS_GROWTH_KIB KiB"
check rss-end-growth \
    ".process.rss_kib.end_growth <= $MAX_RSS_GROWTH_KIB" \
    "$rss_end KiB" "<= $MAX_RSS_GROWTH_KIB KiB"
check rss-steady-slope \
    ".process.rss_kib.last_half_slope_kib_per_hour <= $MAX_RSS_LAST_HALF_SLOPE" \
    "$rss_last_half_slope KiB/hour" "<= $MAX_RSS_LAST_HALF_SLOPE KiB/hour"
check fd-peak-growth \
    ".process.fds.peak_growth <= $MAX_FD_PEAK_GROWTH" \
    "$fd_peak" "<= $MAX_FD_PEAK_GROWTH (2x concurrency + headroom)"
check fd-end-growth \
    ".process.fds.end_growth <= $MAX_FD_END_GROWTH" \
    "$fd_end" "<= $MAX_FD_END_GROWTH"
check rules-loaded \
    '.rules.loaded == .configured.rules' \
    "$(jq -r '.rules.loaded' "$REPORT")" "configured rule count"
check trace-drained \
    '.trace.pending_sessions == 0 and .trace.incomplete_sessions == 0 and .trace.orphan_events == 0 and .trace.queue_bytes == 0' \
    "pending=$(jq -r '.trace.pending_sessions' "$REPORT") orphan=$(jq -r '.trace.orphan_events' "$REPORT")" \
    "zero pending/incomplete/orphan/queue bytes"
check trace-loss \
    '.trace.queue_dropped == 0 and .trace.queue_memory_dropped == 0 and .trace.spill_errors == 0' \
    "queue_dropped=$(jq -r '.trace.queue_dropped' "$REPORT") spill_errors=$(jq -r '.trace.spill_errors' "$REPORT")" \
    "zero queue/memory/spill errors"
check trace-bounds \
    '.trace.sessions <= .trace.max_sessions and .trace.total_memory_bytes <= .trace.memory_budget_bytes' \
    "sessions=$(jq -r '.trace.sessions' "$REPORT") memory=$(jq -r '.trace.total_memory_bytes' "$REPORT")" \
    "configured session and memory budgets"

[ "$failed" -eq 0 ] || exit 1
echo "Soak stability targets passed."
