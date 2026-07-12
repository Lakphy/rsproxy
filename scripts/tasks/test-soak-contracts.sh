#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-soak-contract.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

report() {
    rps=$1
    errors=$2
    rss=$3
    fd_end=$4
    fd_peak=$5
    pending=$6
    queue_dropped=$7
    rules=$8
    jq -n \
        --argjson rps "$rps" \
        --argjson errors "$errors" \
        --argjson rss "$rss" \
        --argjson fd_end "$fd_end" \
        --argjson fd_peak "$fd_peak" \
        --argjson pending "$pending" \
        --argjson queue_dropped "$queue_dropped" \
        --argjson rules "$rules" '
        {
            schema: "rsproxy.soak/v1",
            driver: "oha",
            duration: "90m",
            warmup_duration: "30s",
            started_at_epoch_seconds: 1,
            elapsed_seconds: 5400,
            configured: {
                qps: 1000,
                concurrency: 64,
                rules: 1001,
                sample_interval_seconds: 60
            },
            load: {
                requests: 5400000,
                requests_per_second: $rps,
                success_rate: (if $errors == 0 then 1 else 0.99 end),
                response_bytes: (5400000 * 1024),
                status_200: (5400000 - $errors),
                errors: $errors
            },
            process: {
                samples: 91,
                rss_kib: {
                    start: 20000,
                    end: (20000 + $rss),
                    max: (20000 + $rss),
                    end_growth: $rss,
                    peak_growth: $rss,
                    slope_kib_per_hour: 0,
                    last_half_slope_kib_per_hour: 0
                },
                fds: {
                    start: 20,
                    end: (20 + $fd_end),
                    max: (20 + $fd_peak),
                    end_growth: $fd_end,
                    peak_growth: $fd_peak
                }
            },
            rules: {loaded: $rules},
            trace: {
                sessions: 4096,
                max_sessions: 4096,
                queue_dropped: $queue_dropped,
                queue_memory_dropped: 0,
                queue_bytes: 0,
                pending_sessions: $pending,
                incomplete_sessions: 0,
                orphan_events: 0,
                total_memory_bytes: 1000000,
                memory_budget_bytes: 67108864,
                spill_errors: 0
            }
        }
    '
}

assert_rejected() {
    name=$1
    if "$ROOT/scripts/targets.sh soak" "$TMP_ROOT/$name.json" >/dev/null 2>&1; then
        echo "soak target check accepted $name" >&2
        exit 1
    fi
}

report 950 0 32768 16 144 0 0 1001 >"$TMP_ROOT/pass.json"
jq '.elapsed_seconds = 5399' "$TMP_ROOT/pass.json" >"$TMP_ROOT/elapsed.json"
jq '.load.requests = 4999999 |
    .load.response_bytes = (4999999 * 1024) |
    .load.status_200 = 4999999' \
    "$TMP_ROOT/pass.json" >"$TMP_ROOT/requests.json"
jq '.process.samples = 89' "$TMP_ROOT/pass.json" >"$TMP_ROOT/samples.json"
jq '.process.rss_kib.last_half_slope_kib_per_hour = 1025' \
    "$TMP_ROOT/pass.json" >"$TMP_ROOT/rss-slope.json"
report 899 0 32768 16 144 0 0 1001 >"$TMP_ROOT/rate.json"
report 950 1 32768 16 144 0 0 1001 >"$TMP_ROOT/load.json"
report 950 0 32769 16 144 0 0 1001 >"$TMP_ROOT/rss.json"
report 950 0 32768 17 144 0 0 1001 >"$TMP_ROOT/fd-end.json"
report 950 0 32768 16 145 0 0 1001 >"$TMP_ROOT/fd-peak.json"
report 950 0 32768 16 144 1 0 1001 >"$TMP_ROOT/pending.json"
report 950 0 32768 16 144 0 1 1001 >"$TMP_ROOT/dropped.json"
report 950 0 32768 16 144 0 0 1000 >"$TMP_ROOT/rules.json"

"$ROOT/scripts/targets.sh soak" "$TMP_ROOT/pass.json" >/dev/null
for failure in elapsed requests samples rss-slope rate load rss fd-end fd-peak pending dropped rules; do
    assert_rejected "$failure"
done
echo "Soak schema, correctness, resource, trace, and rule contracts passed."
