#!/bin/sh
set -eu

BASELINE=${1:?usage: check-performance-regression.sh BASELINE CURRENT [TOLERANCE_PERCENT]}
CURRENT=${2:?usage: check-performance-regression.sh BASELINE CURRENT [TOLERANCE_PERCENT]}
TOLERANCE=${3:-10}
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-regression.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

command -v jq >/dev/null 2>&1 || {
    echo "performance regression check requires jq" >&2
    exit 1
}

for report in "$BASELINE" "$CURRENT"; do
    jq -e '
        .schema == "rsproxy.criterion/v1" and
        .unit == "nanoseconds" and
        (.metrics | type == "object" and length > 0) and
        all(.metrics[];
            (.mean_ns | type == "number") and
            (.lower_ns | type == "number") and
            (.upper_ns | type == "number"))
    ' "$report" >/dev/null || {
        echo "invalid criterion report: $report" >&2
        exit 1
    }
done

jq -r '.metrics | keys[]' "$BASELINE" | while IFS= read -r metric; do
    jq -e --arg metric "$metric" '.metrics[$metric] != null' "$CURRENT" >/dev/null || {
        printf '%s\n' "$metric" >>"$TMP_ROOT/missing"
        continue
    }
    baseline=$(jq -r --arg metric "$metric" '.metrics[$metric].mean_ns' "$BASELINE")
    current=$(jq -r --arg metric "$metric" '.metrics[$metric].mean_ns' "$CURRENT")
    jq -n -e \
        --argjson baseline "$baseline" \
        --argjson current "$current" \
        --argjson tolerance "$TOLERANCE" \
        '$current <= ($baseline * (1 + $tolerance / 100))' >/dev/null || {
            jq -nc \
                --arg metric "$metric" \
                --argjson baseline_ns "$baseline" \
                --argjson current_ns "$current" \
                --argjson tolerance_percent "$TOLERANCE" \
                '{metric: $metric, baseline_ns: $baseline_ns, current_ns: $current_ns,
                  change_percent: (($current_ns / $baseline_ns - 1) * 100),
                  tolerance_percent: $tolerance_percent}' \
                >>"$TMP_ROOT/regressions.ndjson"
        }
done

if [ -s "$TMP_ROOT/missing" ]; then
    echo "current report is missing baseline metrics:" >&2
    sed 's/^/  /' "$TMP_ROOT/missing" >&2
    exit 1
fi
if [ -s "$TMP_ROOT/regressions.ndjson" ]; then
    echo "performance regressions exceeded ${TOLERANCE}%:" >&2
    jq -s '.' "$TMP_ROOT/regressions.ndjson" >&2
    exit 1
fi

count=$(jq '.metrics | length' "$BASELINE")
printf 'Compared %s Criterion metrics; no regression exceeded %s%%.\n' "$count" "$TOLERANCE"
