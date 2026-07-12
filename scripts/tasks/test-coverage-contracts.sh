#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
TMP_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-coverage-contract.XXXXXX")
trap 'rm -rf "$TMP_ROOT"' EXIT HUP INT TERM

report() {
    workspace=$1
    rules=$2
    jq -n --argjson workspace "$workspace" --argjson rules "$rules" '
        {
            schema: "rsproxy.coverage/v1",
            source: "cargo-llvm-cov",
            workspace: {lines: 10000, covered: $workspace, percent: ($workspace / 100)},
            rules: {lines: 10000, covered: $rules, percent: ($rules / 100)},
            production_files: 100
        }
    '
}

report 8500 9500 >"$TMP_ROOT/pass.json"
report 8499 9500 >"$TMP_ROOT/workspace-fail.json"
report 8500 9499 >"$TMP_ROOT/rules-fail.json"

"$ROOT/scripts/targets.sh coverage" "$TMP_ROOT/pass.json" >/dev/null
for name in workspace rules; do
    if "$ROOT/scripts/targets.sh coverage" \
        "$TMP_ROOT/${name}-fail.json" >/dev/null 2>&1
    then
        echo "coverage target check accepted a ${name} threshold failure" >&2
        exit 1
    fi
done

echo "Coverage report and threshold contracts passed."
