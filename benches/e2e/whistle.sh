#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
WHISTLE_VERSION=2.10.5
WHISTLE_DIR=${RSPROXY_WHISTLE_DIR:-}
WHISTLE_DRIVER_DIR="$ROOT/benches/e2e/whistle-driver"
WHISTLE_CACHE_DIR=${RSPROXY_WHISTLE_CACHE_DIR:-"$ROOT/target/bench-deps/whistle-$WHISTLE_VERSION"}
WHISTLE_REQUESTS=${RSPROXY_WHISTLE_REQUESTS:-10000}
CONCURRENCY=${RSPROXY_PERF_CONCURRENCY:-16}
WHISTLE_PORT=${RSPROXY_WHISTLE_PORT:-19900}
PERF_REQUESTS=${RSPROXY_PERF_REQUESTS:-50000}
SKIP_BUILD=${RSPROXY_PERF_SKIP_BUILD:-0}
ENFORCE=${RSPROXY_WHISTLE_ENFORCE:-1}
OUTPUT=${RSPROXY_PERF_OUTPUT:-"$ROOT/target/performance/e2e-whistle.json"}
WHISTLE_OUTPUT=${RSPROXY_WHISTLE_OUTPUT:-"$ROOT/target/performance/whistle.json"}
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-whistle.XXXXXX")
ORIGIN_PID=
WHISTLE_PID=
INSTALL_TMP=

cleanup() {
    set +e
    for pid in "$WHISTLE_PID" "$ORIGIN_PID"; do
        if [ -n "$pid" ]; then
            kill "$pid" 2>/dev/null
            wait "$pid" 2>/dev/null
        fi
    done
    if [ -n "$INSTALL_TMP" ]; then
        rm -rf "$INSTALL_TMP"
    fi
    rm -rf "$TMP_ROOT"
}
trap cleanup EXIT HUP INT TERM

fail() {
    printf 'whistle performance: %s\n' "$*" >&2
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

for command in cargo curl jq node oha; do
    command -v "$command" >/dev/null 2>&1 || fail "requires $command"
done
if [ -z "$WHISTLE_DIR" ]; then
    command -v npm >/dev/null 2>&1 || fail "requires npm to install Whistle $WHISTLE_VERSION"
fi
require_positive_integer RSPROXY_WHISTLE_REQUESTS "$WHISTLE_REQUESTS"
require_positive_integer RSPROXY_PERF_CONCURRENCY "$CONCURRENCY"
require_positive_integer RSPROXY_WHISTLE_PORT "$WHISTLE_PORT"
[ "$WHISTLE_PORT" -le 65535 ] || fail 'RSPROXY_WHISTLE_PORT must be at most 65535'
case $ENFORCE in
    0|1) ;;
    *) fail 'RSPROXY_WHISTLE_ENFORCE must be 0 or 1' ;;
esac
if [ -z "$WHISTLE_DIR" ]; then
    [ -f "$WHISTLE_DRIVER_DIR/package.json" ] \
        || fail "Whistle driver package is missing: $WHISTLE_DRIVER_DIR/package.json"
    [ -f "$WHISTLE_DRIVER_DIR/package-lock.json" ] \
        || fail "Whistle driver lock is missing: $WHISTLE_DRIVER_DIR/package-lock.json"
    if [ ! -f "$WHISTLE_CACHE_DIR/node_modules/whistle/bin/whistle.js" ] \
        || ! cmp -s "$WHISTLE_DRIVER_DIR/package-lock.json" "$WHISTLE_CACHE_DIR/package-lock.json"
    then
        cache_parent=$(dirname "$WHISTLE_CACHE_DIR")
        mkdir -p "$cache_parent"
        INSTALL_TMP=$(mktemp -d "$cache_parent/.whistle-install.XXXXXX")
        cp "$WHISTLE_DRIVER_DIR/package.json" "$WHISTLE_DRIVER_DIR/package-lock.json" "$INSTALL_TMP/"
        npm ci --omit=dev --no-audit --no-fund --prefix "$INSTALL_TMP"
        rm -rf "$WHISTLE_CACHE_DIR"
        mv "$INSTALL_TMP" "$WHISTLE_CACHE_DIR"
        INSTALL_TMP=
    fi
    WHISTLE_DIR="$WHISTLE_CACHE_DIR/node_modules/whistle"
fi

[ -f "$WHISTLE_DIR/bin/whistle.js" ] || fail "Whistle source is missing: $WHISTLE_DIR"
node -e 'require.resolve("express", { paths: [process.argv[1]] })' "$WHISTLE_DIR" \
    >/dev/null 2>&1 || fail "Whistle dependencies are missing for $WHISTLE_DIR"
WHISTLE_ACTUAL_VERSION=$(jq -r '.version // empty' "$WHISTLE_DIR/package.json")
[ "$WHISTLE_ACTUAL_VERSION" = "$WHISTLE_VERSION" ] \
    || fail "Whistle must be version $WHISTLE_VERSION, got ${WHISTLE_ACTUAL_VERSION:-unknown}"

cd "$ROOT"
if [ "$SKIP_BUILD" != "1" ]; then
    cargo build --release -p rsproxy --bin rsproxy --example bench_origin --locked
fi
ORIGIN_BIN="$ROOT/target/release/examples/bench_origin"
[ -x "$ORIGIN_BIN" ] || fail 'release bench_origin binary is missing'

"$ORIGIN_BIN" >"$TMP_ROOT/origin.out" 2>"$TMP_ROOT/origin.err" &
ORIGIN_PID=$!
attempt=0
ORIGIN_ADDR=
while [ "$attempt" -lt 200 ]; do
    ORIGIN_ADDR=$(sed -n 's/^origin_addr=//p' "$TMP_ROOT/origin.out" | head -n 1)
    [ -n "$ORIGIN_ADDR" ] && break
    kill -0 "$ORIGIN_PID" 2>/dev/null || {
        cat "$TMP_ROOT/origin.err" >&2
        fail 'origin exited before binding'
    }
    attempt=$((attempt + 1))
    sleep 0.05
done
[ -n "$ORIGIN_ADDR" ] || fail 'origin did not become ready'

node "$WHISTLE_DIR/bin/whistle.js" run \
    -H 127.0.0.1 -p "$WHISTLE_PORT" \
    -D "$TMP_ROOT/whistle-home" -S benchmark \
    -M pureProxy --no-global-plugins --no-prev-options \
    >"$TMP_ROOT/whistle.out" 2>"$TMP_ROOT/whistle.err" &
WHISTLE_PID=$!
attempt=0
TARGET="http://$ORIGIN_ADDR/benchmark"
while [ "$attempt" -lt 300 ]; do
    if curl --silent --show-error --fail --noproxy '' \
        --proxy "http://127.0.0.1:$WHISTLE_PORT" "$TARGET" >/dev/null 2>&1
    then
        break
    fi
    kill -0 "$WHISTLE_PID" 2>/dev/null || {
        cat "$TMP_ROOT/whistle.out" >&2
        cat "$TMP_ROOT/whistle.err" >&2
        fail 'Whistle exited before becoming ready'
    }
    attempt=$((attempt + 1))
    sleep 0.05
done
[ "$attempt" -lt 300 ] || fail 'Whistle did not become ready'

oha --no-tui --no-color --output-format json --http-version 1.1 \
    -n "$WHISTLE_REQUESTS" -c "$CONCURRENCY" \
    -x "http://127.0.0.1:$WHISTLE_PORT" "$TARGET" \
    >"$TMP_ROOT/whistle-oha.json"
jq -e --argjson requests "$WHISTLE_REQUESTS" '
    .summary.successRate == 1 and
    .summary.totalData == ($requests * 1024) and
    (.errorDistribution | length) == 0 and
    .statusCodeDistribution["200"] == $requests
' "$TMP_ROOT/whistle-oha.json" >/dev/null \
    || fail 'Whistle oha report failed exact correctness checks'

mkdir -p "$(dirname "$WHISTLE_OUTPUT")"
jq --arg version "$WHISTLE_ACTUAL_VERSION" \
    --argjson requests "$WHISTLE_REQUESTS" \
    --argjson concurrency "$CONCURRENCY" '
    {
        schema: "rsproxy.whistle.performance/v1",
        version: $version,
        mode: "pureProxy",
        driver: "oha",
        requests: $requests,
        concurrency: $concurrency,
        requests_per_second: .summary.requestsPerSec,
        p50_us: (.latencyPercentiles.p50 * 1000000),
        p99_us: (.latencyPercentiles.p99 * 1000000),
        response_bytes: .summary.totalData,
        success_rate: .summary.successRate,
        errors: .errorDistribution
    }
' "$TMP_ROOT/whistle-oha.json" >"$WHISTLE_OUTPUT"
WHISTLE_RPS=$(jq -r '.requests_per_second' "$WHISTLE_OUTPUT")

kill "$WHISTLE_PID" 2>/dev/null || true
wait "$WHISTLE_PID" 2>/dev/null || true
WHISTLE_PID=
kill "$ORIGIN_PID" 2>/dev/null || true
wait "$ORIGIN_PID" 2>/dev/null || true
ORIGIN_PID=

RSPROXY_PERF_SKIP_BUILD=1 \
RSPROXY_PERF_REQUESTS="$PERF_REQUESTS" \
RSPROXY_PERF_CONCURRENCY="$CONCURRENCY" \
RSPROXY_WHISTLE_RPS="$WHISTLE_RPS" \
RSPROXY_PERF_OUTPUT="$OUTPUT" \
    "$ROOT/benches/e2e/performance.sh"

cat "$WHISTLE_OUTPUT"
if [ "$ENFORCE" = "1" ]; then
    RSPROXY_PERF_REQUIRE_WHISTLE=1 \
        "$ROOT/scripts/check-e2e-performance-targets.sh" "$OUTPUT"
fi
