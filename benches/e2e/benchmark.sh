#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
cd "$ROOT"

REQUESTS=${RSPROXY_BENCH_REQUESTS:-1000}
CONCURRENCY=${RSPROXY_BENCH_CONCURRENCY:-16}
SKIP_BUILD=${RSPROXY_BENCH_SKIP_BUILD:-0}
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-bench.XXXXXX")
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

command -v jq >/dev/null 2>&1 || {
    echo "benchmark requires jq" >&2
    exit 1
}
command -v curl >/dev/null 2>&1 || {
    echo "benchmark requires curl" >&2
    exit 1
}

if [ "$SKIP_BUILD" != "1" ]; then
    cargo build --release -p rsproxy-cli --bin rsproxy --locked
    cargo build --release -p rsproxy-engine \
        --example bench_origin --example bench_client --locked
fi

ORIGIN_BIN="$ROOT/target/release/examples/bench_origin"
CLIENT_BIN="$ROOT/target/release/examples/bench_client"
PROXY_BIN="$ROOT/target/release/rsproxy"

"$ORIGIN_BIN" >"$TMP_ROOT/origin.log" 2>"$TMP_ROOT/origin.err" &
ORIGIN_PID=$!

attempt=0
ORIGIN_ADDR=
while [ "$attempt" -lt 100 ]; do
    ORIGIN_ADDR=$(sed -n 's/^origin_addr=//p' "$TMP_ROOT/origin.log" | head -n 1)
    [ -n "$ORIGIN_ADDR" ] && break
    kill -0 "$ORIGIN_PID" 2>/dev/null || {
        cat "$TMP_ROOT/origin.err" >&2
        exit 1
    }
    attempt=$((attempt + 1))
    sleep 0.1
done
[ -n "$ORIGIN_ADDR" ] || {
    echo "benchmark origin did not become ready" >&2
    exit 1
}

mkdir -p "$TMP_ROOT/home" "$TMP_ROOT/storage"
HOME="$TMP_ROOT/home" \
RSPROXY_HOME="$TMP_ROOT/home" \
RSPROXY_LOG="rsproxy_cli=info" \
RSPROXY_LOG_FORMAT=json \
"$PROXY_BIN" run \
    --host 127.0.0.1 \
    --port 0 \
    --api 127.0.0.1:0 \
    --storage "$TMP_ROOT/storage" \
    --no-mitm \
    --trace-body-limit 0 \
    --trace-disk-budget 0 \
    >"$TMP_ROOT/proxy.out" 2>"$TMP_ROOT/proxy.log" &
PROXY_PID=$!

attempt=0
PROXY_ADDR=
while [ "$attempt" -lt 100 ]; do
    PROXY_ADDR=$(jq -r \
        'select(.fields.event == "proxy_listener_bound") | .fields.address' \
        "$TMP_ROOT/proxy.log" 2>/dev/null | head -n 1)
    [ -n "$PROXY_ADDR" ] && [ "$PROXY_ADDR" != "null" ] && break
    kill -0 "$PROXY_PID" 2>/dev/null || {
        cat "$TMP_ROOT/proxy.log" >&2
        exit 1
    }
    attempt=$((attempt + 1))
    sleep 0.1
done
[ -n "$PROXY_ADDR" ] && [ "$PROXY_ADDR" != "null" ] || {
    echo "rsproxy did not become ready" >&2
    cat "$TMP_ROOT/proxy.log" >&2
    exit 1
}

SMOKE_BYTES=$(curl --silent --show-error --noproxy "" \
    --proxy "http://$PROXY_ADDR" "http://$ORIGIN_ADDR/smoke" | wc -c | tr -d ' ')
[ "$SMOKE_BYTES" = "1024" ] || {
    echo "benchmark smoke expected 1024 bytes, got $SMOKE_BYTES" >&2
    exit 1
}

echo "benchmark proxy=$PROXY_ADDR origin=$ORIGIN_ADDR requests=$REQUESTS concurrency=$CONCURRENCY smoke_bytes=$SMOKE_BYTES" >&2
"$CLIENT_BIN" \
    --proxy "$PROXY_ADDR" \
    --target "http://$ORIGIN_ADDR/benchmark" \
    --requests "$REQUESTS" \
    --concurrency "$CONCURRENCY"
