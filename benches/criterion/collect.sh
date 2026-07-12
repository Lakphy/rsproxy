#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
CRITERION_DIR=${RSPROXY_CRITERION_DIR:-${CARGO_TARGET_DIR:-"$ROOT/target"}/criterion}
OUTPUT=${1:-"$ROOT/target/performance/criterion.json"}
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-criterion.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

command -v jq >/dev/null 2>&1 || {
    echo "criterion report collection requires jq" >&2
    exit 1
}
[ -d "$CRITERION_DIR" ] || {
    echo "criterion result directory not found: $CRITERION_DIR" >&2
    exit 1
}

find "$CRITERION_DIR" -type f -path '*/new/estimates.json' -print \
    | LC_ALL=C sort \
    | while IFS= read -r estimate; do
        relative=${estimate#"$CRITERION_DIR/"}
        metric=${relative%/new/estimates.json}
        jq -c --arg metric "$metric" '
            {
                key: $metric,
                value: {
                    mean_ns: .mean.point_estimate,
                    lower_ns: .mean.confidence_interval.lower_bound,
                    upper_ns: .mean.confidence_interval.upper_bound
                }
            }
        ' "$estimate"
    done >"$TMP_ROOT/entries.ndjson"

[ -s "$TMP_ROOT/entries.ndjson" ] || {
    echo "no criterion estimates found under $CRITERION_DIR" >&2
    exit 1
}

mkdir -p "$(dirname "$OUTPUT")"
jq -s '
    {
        schema: "rsproxy.criterion/v1",
        unit: "nanoseconds",
        metrics: from_entries
    }
' "$TMP_ROOT/entries.ndjson" >"$TMP_ROOT/report.json"
mv "$TMP_ROOT/report.json" "$OUTPUT"
printf '%s\n' "$OUTPUT"
