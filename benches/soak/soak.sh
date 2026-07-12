#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
DURATION=${RSPROXY_SOAK_DURATION:-90m}
WARMUP_DURATION=${RSPROXY_SOAK_WARMUP_DURATION:-30s}
QPS=${RSPROXY_SOAK_QPS:-1000}
CONCURRENCY=${RSPROXY_SOAK_CONCURRENCY:-64}
RULES=${RSPROXY_SOAK_RULES:-1000}
SAMPLE_INTERVAL=${RSPROXY_SOAK_SAMPLE_INTERVAL_SECONDS:-60}
SKIP_BUILD=${RSPROXY_SOAK_SKIP_BUILD:-0}
OUTPUT=${RSPROXY_SOAK_OUTPUT:-"$ROOT/target/soak/report.json"}
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-soak.XXXXXX")
ORIGIN_PID=
PROXY_PID=
LOAD_PID=
SAMPLER_PID=

cleanup() {
    set +e
    for pid in "$SAMPLER_PID" "$LOAD_PID" "$PROXY_PID" "$ORIGIN_PID"; do
        if [ -n "$pid" ]; then
            kill "$pid" 2>/dev/null
            wait "$pid" 2>/dev/null
        fi
    done
    rm -rf "$TMP_ROOT"
}
trap cleanup EXIT HUP INT TERM

fail() {
    echo "soak: $*" >&2
    exit 1
}

require_positive_integer() {
    name=$1
    value=$2
    case $value in
        ''|*[!0-9]*) fail "$name must be a positive integer, got $value" ;;
    esac
    [ "$value" -gt 0 ] || fail "$name must be greater than zero"
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
    value=$(ps -o rss= -p "$1" | tr -d ' ')
    [ -n "$value" ] || fail "process $1 disappeared while sampling RSS"
    printf '%s\n' "$value"
}

process_fd_count() {
    pid=$1
    if [ -d "/proc/$pid/fd" ]; then
        find "/proc/$pid/fd" -mindepth 1 -maxdepth 1 -print | wc -l | tr -d ' '
        return
    fi
    lsof -n -P -p "$pid" 2>/dev/null \
        | awk 'NR > 1 && $4 ~ /^[0-9]+[A-Za-z]*$/ { count++ } END { print count + 0 }'
}

sample_process() {
    now=$(date +%s)
    rss=$(process_rss_kib "$PROXY_PID")
    fds=$(process_fd_count "$PROXY_PID")
    printf '%s\t%s\t%s\n' "$now" "$rss" "$fds" >>"$TMP_ROOT/samples.tsv"
}

validate_oha_report() {
    report=$1
    phase=$2
    jq -e '
        ([.statusCodeDistribution[]] | add // 0) as $requests |
        .summary.successRate == 1 and
        $requests > 0 and
        .statusCodeDistribution["200"] == $requests and
        .summary.totalData == ($requests * 1024) and
        (.errorDistribution | length) == 0
    ' "$report" >/dev/null || {
        echo "soak $phase report failed correctness checks" >&2
        jq '.' "$report" >&2
        exit 1
    }
}

api_status() {
    curl --silent --show-error --fail --noproxy '*' \
        --header "Authorization: Bearer $API_TOKEN" \
        "http://$CONTROL_ADDR/api/status"
}

for command in cargo curl find jq oha ps; do
    command -v "$command" >/dev/null 2>&1 || fail "requires $command"
done
if [ ! -d /proc/$$/fd ]; then
    command -v lsof >/dev/null 2>&1 || fail "requires lsof when /proc is unavailable"
fi
require_positive_integer RSPROXY_SOAK_QPS "$QPS"
require_positive_integer RSPROXY_SOAK_CONCURRENCY "$CONCURRENCY"
require_positive_integer RSPROXY_SOAK_RULES "$RULES"
require_positive_integer RSPROXY_SOAK_SAMPLE_INTERVAL_SECONDS "$SAMPLE_INTERVAL"

cd "$ROOT"
if [ "$SKIP_BUILD" != "1" ]; then
    cargo build --release -p rsproxy --bin rsproxy --example bench_origin --locked
fi

PROXY_BIN="$ROOT/target/release/rsproxy"
ORIGIN_BIN="$ROOT/target/release/examples/bench_origin"
[ -x "$PROXY_BIN" ] || fail "release rsproxy binary is missing"
[ -x "$ORIGIN_BIN" ] || fail "release bench_origin binary is missing"

mkdir -p "$TMP_ROOT/home" "$TMP_ROOT/storage/rules"
RULE_FILE="$TMP_ROOT/storage/rules/default.rules"
i=0
while [ "$i" -lt "$RULES" ]; do
    case $((i % 5)) in
        0) printf 'unused-%s.soak.invalid tag(exact-%s)\n' "$i" "$i" ;;
        1) printf '**.unused-%s.soak.invalid tag(suffix-%s)\n' "$i" "$i" ;;
        2) printf '/unused-path-%s/' "$i"; printf ' tag(regex-%s)\n' "$i" ;;
        3) printf ':65534 tag(port-%s) when method(POST)\n' "$i" ;;
        4) printf 'unused-%s.soak.invalid req.header(x-soak-%s: idle)\n' "$i" "$i" ;;
    esac
    i=$((i + 1))
done >"$RULE_FILE"
printf '127.0.0.1 res.header(x-rsproxy-soak: active) tag(soak-live) when method(GET)\n' \
    >>"$RULE_FILE"
RULE_COUNT=$((RULES + 1))
"$PROXY_BIN" rules check --file "$RULE_FILE" --storage "$TMP_ROOT/storage" >/dev/null

"$ORIGIN_BIN" >"$TMP_ROOT/origin.out" 2>"$TMP_ROOT/origin.err" &
ORIGIN_PID=$!
attempt=0
ORIGIN_ADDR=
while [ "$attempt" -lt 200 ]; do
    ORIGIN_ADDR=$(sed -n 's/^origin_addr=//p' "$TMP_ROOT/origin.out" | head -n 1)
    [ -n "$ORIGIN_ADDR" ] && break
    kill -0 "$ORIGIN_PID" 2>/dev/null || {
        cat "$TMP_ROOT/origin.err" >&2
        fail "origin exited before binding"
    }
    attempt=$((attempt + 1))
    sleep 0.05
done
[ -n "$ORIGIN_ADDR" ] || fail "origin did not become ready"

HOME="$TMP_ROOT/home" \
RSPROXY_HOME="$TMP_ROOT/home" \
RSPROXY_LOG="rsproxy=info" \
RSPROXY_LOG_FORMAT=json \
"$PROXY_BIN" run \
    --host 127.0.0.1 \
    --port 0 \
    --api 127.0.0.1:0 \
    --storage "$TMP_ROOT/storage" \
    --no-mitm \
    --trace-body-limit 64 \
    --trace-mem-budget 64mb \
    --trace-disk-budget 0 \
    >"$TMP_ROOT/proxy.out" 2>"$TMP_ROOT/proxy.err" &
PROXY_PID=$!
PROXY_ADDR=$(wait_for_json_field \
    "$PROXY_PID" "$TMP_ROOT/proxy.err" proxy_listener_bound address)
CONTROL_ADDR=$(wait_for_json_field \
    "$PROXY_PID" "$TMP_ROOT/proxy.err" control_listener_bound address)

API_TOKEN_PATH="$TMP_ROOT/storage/run/api-token"
attempt=0
while [ "$attempt" -lt 200 ] && [ ! -s "$API_TOKEN_PATH" ]; do
    kill -0 "$PROXY_PID" 2>/dev/null || fail "proxy exited before publishing API token"
    attempt=$((attempt + 1))
    sleep 0.05
done
[ -s "$API_TOKEN_PATH" ] || fail "API token was not created"
API_TOKEN=$(cat "$API_TOKEN_PATH")

TARGET="http://$ORIGIN_ADDR/soak"
curl --silent --show-error --fail --noproxy '' \
    --proxy "http://$PROXY_ADDR" "$TARGET" >/dev/null
oha --no-tui --no-color --output-format json --http-version 1.1 --wait-ongoing-requests-after-deadline \
    -z "$WARMUP_DURATION" -q "$QPS" -c "$CONCURRENCY" \
    -x "http://$PROXY_ADDR" "$TARGET" >"$TMP_ROOT/warmup.json"
validate_oha_report "$TMP_ROOT/warmup.json" warmup

printf 'timestamp\trss_kib\tfds\n' >"$TMP_ROOT/samples.tsv"
sample_process
STARTED_AT=$(date +%s)
oha --no-tui --no-color --output-format json --http-version 1.1 --wait-ongoing-requests-after-deadline \
    -z "$DURATION" -q "$QPS" -c "$CONCURRENCY" \
    -x "http://$PROXY_ADDR" "$TARGET" >"$TMP_ROOT/load.json" &
LOAD_PID=$!
(
    while kill -0 "$LOAD_PID" 2>/dev/null; do
        sleep "$SAMPLE_INTERVAL"
        kill -0 "$LOAD_PID" 2>/dev/null || break
        sample_process
    done
) &
SAMPLER_PID=$!

set +e
wait "$LOAD_PID"
LOAD_STATUS=$?
set -e
LOAD_PID=
kill "$SAMPLER_PID" 2>/dev/null || true
wait "$SAMPLER_PID" 2>/dev/null || true
SAMPLER_PID=
[ "$LOAD_STATUS" -eq 0 ] || fail "oha exited with status $LOAD_STATUS"
sample_process
FINISHED_AT=$(date +%s)
validate_oha_report "$TMP_ROOT/load.json" load

attempt=0
while [ "$attempt" -lt 200 ]; do
    api_status >"$TMP_ROOT/status.json"
    if jq -e '.trace.pending_sessions == 0 and .trace.queue_bytes == 0' \
        "$TMP_ROOT/status.json" >/dev/null
    then
        break
    fi
    attempt=$((attempt + 1))
    sleep 0.05
done
jq -e '.trace.pending_sessions == 0 and .trace.queue_bytes == 0' \
    "$TMP_ROOT/status.json" >/dev/null || fail "trace collector did not drain"

START_RSS=$(awk 'NR == 2 { print $2 }' "$TMP_ROOT/samples.tsv")
END_RSS=$(awk 'END { print $2 }' "$TMP_ROOT/samples.tsv")
MAX_RSS=$(awk 'NR > 1 && $2 > max { max = $2 } END { print max + 0 }' "$TMP_ROOT/samples.tsv")
START_FDS=$(awk 'NR == 2 { print $3 }' "$TMP_ROOT/samples.tsv")
END_FDS=$(awk 'END { print $3 }' "$TMP_ROOT/samples.tsv")
MAX_FDS=$(awk 'NR > 1 && $3 > max { max = $3 } END { print max + 0 }' "$TMP_ROOT/samples.tsv")
SAMPLES=$(awk 'END { print NR - 1 }' "$TMP_ROOT/samples.tsv")
RSS_SLOPE_KIB_PER_HOUR=$(awk '
    NR == 2 { started = $1 }
    NR > 1 {
        x = $1 - started
        y = $2
        count++
        sum_x += x
        sum_y += y
        sum_xx += x * x
        sum_xy += x * y
    }
    END {
        denominator = count * sum_xx - sum_x * sum_x
        if (count < 2 || denominator == 0) print 0
        else print (count * sum_xy - sum_x * sum_y) / denominator * 3600
    }
' "$TMP_ROOT/samples.tsv")
HALF_SAMPLES=$(((SAMPLES + 1) / 2))
RSS_LAST_HALF_SLOPE_KIB_PER_HOUR=$(tail -n "$HALF_SAMPLES" "$TMP_ROOT/samples.tsv" | awk '
    NR == 1 { started = $1 }
    {
        x = $1 - started
        y = $2
        count++
        sum_x += x
        sum_y += y
        sum_xx += x * x
        sum_xy += x * y
    }
    END {
        denominator = count * sum_xx - sum_x * sum_x
        if (count < 2 || denominator == 0) print 0
        else print (count * sum_xy - sum_x * sum_y) / denominator * 3600
    }
')

mkdir -p "$(dirname "$OUTPUT")"
jq -n \
    --slurpfile load "$TMP_ROOT/load.json" \
    --slurpfile status "$TMP_ROOT/status.json" \
    --arg duration "$DURATION" \
    --arg warmup_duration "$WARMUP_DURATION" \
    --argjson started_at "$STARTED_AT" \
    --argjson finished_at "$FINISHED_AT" \
    --argjson qps "$QPS" \
    --argjson concurrency "$CONCURRENCY" \
    --argjson rules "$RULE_COUNT" \
    --argjson sample_interval "$SAMPLE_INTERVAL" \
    --argjson samples "$SAMPLES" \
    --argjson rss_slope "$RSS_SLOPE_KIB_PER_HOUR" \
    --argjson rss_last_half_slope "$RSS_LAST_HALF_SLOPE_KIB_PER_HOUR" \
    --argjson start_rss "$START_RSS" \
    --argjson end_rss "$END_RSS" \
    --argjson max_rss "$MAX_RSS" \
    --argjson start_fds "$START_FDS" \
    --argjson end_fds "$END_FDS" \
    --argjson max_fds "$MAX_FDS" '
    ($load[0]) as $result |
    ([ $result.statusCodeDistribution[] ] | add // 0) as $requests |
    {
        schema: "rsproxy.soak/v1",
        driver: "oha",
        duration: $duration,
        warmup_duration: $warmup_duration,
        started_at_epoch_seconds: $started_at,
        elapsed_seconds: ($finished_at - $started_at),
        configured: {
            qps: $qps,
            concurrency: $concurrency,
            rules: $rules,
            sample_interval_seconds: $sample_interval
        },
        load: {
            requests: $requests,
            requests_per_second: $result.summary.requestsPerSec,
            success_rate: $result.summary.successRate,
            response_bytes: $result.summary.totalData,
            status_200: ($result.statusCodeDistribution["200"] // 0),
            errors: ($result.errorDistribution | to_entries | map(.value) | add // 0)
        },
        process: {
            samples: $samples,
            rss_kib: {
                start: $start_rss,
                end: $end_rss,
                max: $max_rss,
                end_growth: ([0, ($end_rss - $start_rss)] | max),
                peak_growth: ([0, ($max_rss - $start_rss)] | max),
                slope_kib_per_hour: $rss_slope,
                last_half_slope_kib_per_hour: $rss_last_half_slope
            },
            fds: {
                start: $start_fds,
                end: $end_fds,
                max: $max_fds,
                end_growth: ([0, ($end_fds - $start_fds)] | max),
                peak_growth: ([0, ($max_fds - $start_fds)] | max)
            }
        },
        rules: {loaded: $status[0].rules},
        trace: $status[0].trace
    }
' >"$OUTPUT"

cat "$OUTPUT"
"$ROOT/scripts/check-soak-targets.sh" "$OUTPUT"
