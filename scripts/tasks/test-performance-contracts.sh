#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-performance-contract.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

report() {
    value=$1
    printf '%s\n' \
        "{\"schema\":\"rsproxy.criterion/v1\",\"unit\":\"nanoseconds\",\"metrics\":{\"sample\":{\"mean_ns\":$value,\"lower_ns\":$value,\"upper_ns\":$value}}}"
}

report 100 >"$TMP_ROOT/baseline.json"
report 110 >"$TMP_ROOT/within.json"
report 111 >"$TMP_ROOT/regressed.json"
printf '%s\n' \
    '{"schema":"rsproxy.criterion/v1","unit":"nanoseconds","metrics":{"other":{"mean_ns":1,"lower_ns":1,"upper_ns":1}}}' \
    >"$TMP_ROOT/missing.json"

e2e_report() {
    rps=$1
    p50=$2
    p99=$3
    rss=$4
    whistle=$5
    jq -n \
        --argjson rps "$rps" \
        --argjson p50 "$p50" \
        --argjson p99 "$p99" \
        --argjson rss "$rss" \
        --argjson whistle "$whistle" '
        {
            schema: "rsproxy.e2e.performance/v1",
            driver: "oha",
            requests: 50000,
            concurrency: 32,
            direct: {
                requests_per_second: 120000,
                p50_us: 80,
                p99_us: 250,
                response_bytes: 51200000
            },
            proxy: {
                requests_per_second: $rps,
                p50_us: 250,
                p99_us: 1800,
                response_bytes: 51200000
            },
            added_latency: {p50_us: $p50, p99_us: $p99},
            memory: {
                empty_rss_kib: $rss,
                full_trace_rss_kib: 200000,
                growth_kib: 180000
            },
            whistle: {
                requests_per_second: ($rps / $whistle),
                speedup: $whistle
            }
        }
    '
}

e2e_report 80000 299 1999 30719 10 >"$TMP_ROOT/e2e-pass.json"
e2e_report 79999 299 1999 30719 10 >"$TMP_ROOT/e2e-rps-fail.json"
e2e_report 80000 300 1999 30719 10 >"$TMP_ROOT/e2e-p50-fail.json"
e2e_report 80000 299 2000 30719 10 >"$TMP_ROOT/e2e-p99-fail.json"
e2e_report 80000 299 1999 30720 10 >"$TMP_ROOT/e2e-rss-fail.json"
e2e_report 80000 299 1999 30719 9.99 >"$TMP_ROOT/e2e-whistle-fail.json"

criterion_target_report() {
    upper=$1
    jq -n --argjson upper "$upper" '
        {
            schema: "rsproxy.criterion/v1",
            unit: "nanoseconds",
            metrics: {
                "mitm_certificate/cached_tls_handshake": {
                    mean_ns: ($upper - 1000),
                    lower_ns: ($upper - 2000),
                    upper_ns: $upper
                }
            }
        }
    '
}

criterion_target_report 2999999 >"$TMP_ROOT/tls-pass.json"
criterion_target_report 3000000 >"$TMP_ROOT/tls-fail.json"

"$ROOT/scripts/targets.sh regression" \
    "$TMP_ROOT/baseline.json" "$TMP_ROOT/within.json" 10 >/dev/null
if "$ROOT/scripts/targets.sh regression" \
    "$TMP_ROOT/baseline.json" "$TMP_ROOT/regressed.json" 10 >/dev/null 2>&1
then
    echo "regression comparator accepted an 11% slowdown" >&2
    exit 1
fi
if "$ROOT/scripts/targets.sh regression" \
    "$TMP_ROOT/baseline.json" "$TMP_ROOT/missing.json" 10 >/dev/null 2>&1
then
    echo "regression comparator accepted a missing metric" >&2
    exit 1
fi

"$ROOT/scripts/targets.sh e2e" \
    "$TMP_ROOT/e2e-pass.json" >/dev/null
env RSPROXY_PERF_REQUIRE_WHISTLE=1 \
    "$ROOT/scripts/targets.sh e2e" \
    "$TMP_ROOT/e2e-pass.json" >/dev/null
for report_name in rps p50 p99 rss; do
    if "$ROOT/scripts/targets.sh e2e" \
        "$TMP_ROOT/e2e-${report_name}-fail.json" >/dev/null 2>&1
    then
        echo "e2e target check accepted a ${report_name} threshold failure" >&2
        exit 1
    fi
done
if env RSPROXY_PERF_REQUIRE_WHISTLE=1 \
    "$ROOT/scripts/targets.sh e2e" \
    "$TMP_ROOT/e2e-whistle-fail.json" >/dev/null 2>&1
then
    echo "e2e target check accepted a Whistle speedup below 10x" >&2
    exit 1
fi
"$ROOT/scripts/targets.sh criterion" \
    "$TMP_ROOT/tls-pass.json" >/dev/null
if "$ROOT/scripts/targets.sh criterion" \
    "$TMP_ROOT/tls-fail.json" >/dev/null 2>&1
then
    echo "Criterion target check accepted a 3ms TLS handshake" >&2
    exit 1
fi

echo "Performance reports, absolute targets, and 10% regression contracts passed."
