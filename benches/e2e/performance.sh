#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
REQUESTS=${RSPROXY_PERF_REQUESTS:-50000}
CONCURRENCY=${RSPROXY_PERF_CONCURRENCY:-32}
SKIP_BUILD=${RSPROXY_PERF_SKIP_BUILD:-0}
OUTPUT=${RSPROXY_PERF_OUTPUT:-"$ROOT/target/performance/e2e.json"}
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-performance.XXXXXX")
ORIGIN_PID=
PROXY_PID=

cleanup() {
    set +e
    if [ -n "$PROXY_PID" ]; then
        kill "$PROXY_PID" 2>/dev/null
        wait "$PROXY_PID" 2>/dev/null
    fi
    if [ -n "$ORIGIN_PID" ]; then
        kill "$ORIGIN_PID" 2>/dev/null
        wait "$ORIGIN_PID" 2>/dev/null
    fi
    rm -rf "$TMP_ROOT"
}
trap cleanup EXIT HUP INT TERM

wait_for_value() {
    pid=$1
    file=$2
    pattern=$3
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        value=$(grep -E "$pattern" "$file" 2>/dev/null | head -n 1 || true)
        if [ -n "$value" ]; then
            printf '%s\n' "$value"
            return 0
        fi
        kill -0 "$pid" 2>/dev/null || {
            cat "$file" >&2
            return 1
        }
        attempt=$((attempt + 1))
        sleep 0.05
    done
    echo "timed out waiting for $pattern in $file" >&2
    return 1
}

wait_for_json_field() {
    pid=$1
    file=$2
    event=$3
    field=$4
    attempt=0
    while [ "$attempt" -lt 200 ]; do
        value=$(jq -r \
            --arg event "$event" --arg field "$field" \
            'select(.fields.event == $event) | .fields[$field]' \
            "$file" 2>/dev/null | head -n 1 || true)
        if [ -n "$value" ] && [ "$value" != "null" ]; then
            printf '%s\n' "$value"
            return 0
        fi
        kill -0 "$pid" 2>/dev/null || {
            cat "$file" >&2
            return 1
        }
        attempt=$((attempt + 1))
        sleep 0.05
    done
    echo "timed out waiting for $event.$field" >&2
    return 1
}

process_rss_kib() {
    ps -o rss= -p "$1" | tr -d ' '
}

for command in jq curl oha ps; do
    command -v "$command" >/dev/null 2>&1 || {
        echo "e2e performance benchmark requires $command" >&2
        exit 1
    }
done

cd "$ROOT"
if [ "$SKIP_BUILD" != "1" ]; then
    cargo build --release -p rsproxy --bin rsproxy --example bench_origin --locked
fi

"$ROOT/target/release/examples/bench_origin" \
    >"$TMP_ROOT/origin.out" 2>"$TMP_ROOT/origin.err" &
ORIGIN_PID=$!
ORIGIN_ADDR=$(wait_for_value "$ORIGIN_PID" "$TMP_ROOT/origin.out" '^origin_addr=')
ORIGIN_ADDR=${ORIGIN_ADDR#origin_addr=}

mkdir -p "$TMP_ROOT/home" "$TMP_ROOT/storage"
HOME="$TMP_ROOT/home" \
RSPROXY_HOME="$TMP_ROOT/home" \
RSPROXY_LOG="rsproxy=info" \
RSPROXY_LOG_FORMAT=json \
"$ROOT/target/release/rsproxy" run \
    --host 127.0.0.1 \
    --port 0 \
    --api 127.0.0.1:0 \
    --storage "$TMP_ROOT/storage" \
    --no-mitm \
    --trace-body-limit 0 \
    --trace-disk-budget 0 \
    >"$TMP_ROOT/proxy.out" 2>"$TMP_ROOT/proxy.err" &
PROXY_PID=$!
PROXY_ADDR=$(wait_for_json_field \
    "$PROXY_PID" "$TMP_ROOT/proxy.err" proxy_listener_bound address)

curl --silent --show-error "http://$ORIGIN_ADDR/warmup" >/dev/null
curl --silent --show-error --noproxy "" \
    --proxy "http://$PROXY_ADDR" "http://$ORIGIN_ADDR/warmup" >/dev/null
EMPTY_RSS_KIB=$(process_rss_kib "$PROXY_PID")

oha --no-tui --no-color --output-format json --http-version 1.1 \
    -n "$REQUESTS" -c "$CONCURRENCY" \
    "http://$ORIGIN_ADDR/benchmark" >"$TMP_ROOT/direct.json"
oha --no-tui --no-color --output-format json --http-version 1.1 \
    -n "$REQUESTS" -c "$CONCURRENCY" \
    -x "http://$PROXY_ADDR" "http://$ORIGIN_ADDR/benchmark" \
    >"$TMP_ROOT/proxy.json"
sleep 1
FULL_RSS_KIB=$(process_rss_kib "$PROXY_PID")

for report in "$TMP_ROOT/direct.json" "$TMP_ROOT/proxy.json"; do
    jq -e --argjson requests "$REQUESTS" '
        .summary.successRate == 1 and
        .summary.totalData == ($requests * 1024) and
        (.errorDistribution | length) == 0 and
        .statusCodeDistribution["200"] == $requests
    ' "$report" >/dev/null || {
        echo "oha report failed correctness checks: $report" >&2
        jq '.' "$report" >&2
        exit 1
    }
done

mkdir -p "$(dirname "$OUTPUT")"
jq -n \
    --slurpfile direct "$TMP_ROOT/direct.json" \
    --slurpfile proxy "$TMP_ROOT/proxy.json" \
    --argjson requests "$REQUESTS" \
    --argjson concurrency "$CONCURRENCY" \
    --argjson empty_rss_kib "$EMPTY_RSS_KIB" \
    --argjson full_rss_kib "$FULL_RSS_KIB" \
    --arg whistle_rps "${RSPROXY_WHISTLE_RPS:-}" '
    def micros: . * 1000000;
    def metric($report): {
        requests_per_second: $report[0].summary.requestsPerSec,
        p50_us: ($report[0].latencyPercentiles.p50 | micros),
        p99_us: ($report[0].latencyPercentiles.p99 | micros),
        response_bytes: $report[0].summary.totalData
    };
    (metric($direct)) as $direct_metric |
    (metric($proxy)) as $proxy_metric |
    {
        schema: "rsproxy.e2e.performance/v1",
        driver: "oha",
        requests: $requests,
        concurrency: $concurrency,
        direct: $direct_metric,
        proxy: $proxy_metric,
        added_latency: {
            p50_us: ([0, ($proxy_metric.p50_us - $direct_metric.p50_us)] | max),
            p99_us: ([0, ($proxy_metric.p99_us - $direct_metric.p99_us)] | max)
        },
        memory: {
            empty_rss_kib: $empty_rss_kib,
            full_trace_rss_kib: $full_rss_kib,
            growth_kib: ([0, ($full_rss_kib - $empty_rss_kib)] | max)
        },
        whistle: (if $whistle_rps == "" then null else {
            requests_per_second: ($whistle_rps | tonumber),
            speedup: ($proxy_metric.requests_per_second / ($whistle_rps | tonumber))
        } end)
    }
' >"$OUTPUT"

cat "$OUTPUT"
