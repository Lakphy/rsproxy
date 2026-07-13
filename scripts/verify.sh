#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$SCRIPT_DIR/lib.sh"
ROOT=$(repo_root "$0")

run_verify() {
    command=$1
    shift
    case $command in
        actions) run_script "$ROOT" test-action-effects.sh "$@" ;;
        matrix) run_script "$ROOT" test-protocol-matrix.sh "$@" ;;
        bench) run_script "$ROOT" test-benchmark.sh "$@" ;;
        package) run_script "$ROOT" test-npm-package.sh "$@" ;;
        stream) run_script "$ROOT" test-large-stream-resource.sh "$@" ;;
        coverage-report) run_script "$ROOT" coverage.sh "$@" ;;
        fuzz) run_script "$ROOT" fuzz-rules-smoke.sh "$@" ;;
        *)
            usage_error 'usage: scripts/verify.sh <actions|matrix|bench|package|stream|coverage-report|fuzz|all> [args...]'
            ;;
    esac
}

command=${1:-all}
shift $(( $# > 0 ? 1 : 0 ))
case $command in
    all)
        [ "$#" -eq 0 ] || usage_error 'scripts/verify.sh all does not accept arguments'
        for verify in actions matrix bench package stream; do
            run_verify "$verify"
        done
        ;;
    *) run_verify "$command" "$@" ;;
esac
