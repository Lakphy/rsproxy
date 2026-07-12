#!/bin/sh
set -eu

REPORT=${1:?usage: check-coverage-targets.sh REPORT}
MIN_WORKSPACE=${RSPROXY_COVERAGE_MIN_WORKSPACE:-85}
MIN_RULES=${RSPROXY_COVERAGE_MIN_RULES:-95}

command -v jq >/dev/null 2>&1 || {
    echo "coverage target check requires jq" >&2
    exit 1
}

jq -e '
    .schema == "rsproxy.coverage/v1" and
    all(.workspace, .rules;
        (.lines | type == "number" and . > 0) and
        (.covered | type == "number" and . >= 0) and
        .covered <= .lines and
        (.percent | type == "number" and . >= 0 and . <= 100))
' "$REPORT" >/dev/null || {
    echo "invalid coverage report: $REPORT" >&2
    exit 1
}

workspace=$(jq -r '.workspace.percent' "$REPORT")
rules=$(jq -r '.rules.percent' "$REPORT")
failed=0

if jq -e --argjson minimum "$MIN_WORKSPACE" \
    '.workspace.percent >= $minimum' "$REPORT" >/dev/null
then
    printf 'PASS workspace-lines observed=%s%% target=>=%s%%\n' "$workspace" "$MIN_WORKSPACE"
else
    printf 'FAIL workspace-lines observed=%s%% target=>=%s%%\n' \
        "$workspace" "$MIN_WORKSPACE" >&2
    failed=1
fi

if jq -e --argjson minimum "$MIN_RULES" \
    '.rules.percent >= $minimum' "$REPORT" >/dev/null
then
    printf 'PASS rules-lines     observed=%s%% target=>=%s%%\n' "$rules" "$MIN_RULES"
else
    printf 'FAIL rules-lines     observed=%s%% target=>=%s%%\n' \
        "$rules" "$MIN_RULES" >&2
    failed=1
fi

[ "$failed" -eq 0 ]
