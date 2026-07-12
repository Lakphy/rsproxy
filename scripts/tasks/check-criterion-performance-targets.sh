#!/bin/sh
set -eu

REPORT=${1:?usage: check-criterion-performance-targets.sh REPORT}
MAX_TLS_HANDSHAKE_NS=${RSPROXY_PERF_MAX_TLS_HANDSHAKE_NS:-3000000}
METRIC=mitm_certificate/cached_tls_handshake

command -v jq >/dev/null 2>&1 || {
    echo "Criterion target check requires jq" >&2
    exit 1
}

jq -e --arg metric "$METRIC" '
    .schema == "rsproxy.criterion/v1" and
    .unit == "nanoseconds" and
    (.metrics[$metric].mean_ns | type == "number" and . > 0) and
    (.metrics[$metric].lower_ns | type == "number" and . > 0) and
    (.metrics[$metric].upper_ns | type == "number" and . > 0) and
    .metrics[$metric].lower_ns <= .metrics[$metric].mean_ns and
    .metrics[$metric].mean_ns <= .metrics[$metric].upper_ns
' "$REPORT" >/dev/null || {
    echo "invalid or incomplete Criterion target report: $REPORT" >&2
    exit 1
}

upper_ns=$(jq -r --arg metric "$METRIC" '.metrics[$metric].upper_ns' "$REPORT")
if ! jq -e \
    --arg metric "$METRIC" \
    --argjson maximum "$MAX_TLS_HANDSHAKE_NS" \
    '.metrics[$metric].upper_ns < $maximum' "$REPORT" >/dev/null
then
    printf 'FAIL cached-tls-handshake observed=%sns target=<%sns\n' \
        "$upper_ns" "$MAX_TLS_HANDSHAKE_NS" >&2
    exit 1
fi

printf 'PASS cached-tls-handshake observed=%sns target=<%sns\n' \
    "$upper_ns" "$MAX_TLS_HANDSHAKE_NS"
