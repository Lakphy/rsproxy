#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$SCRIPT_DIR/lib.sh"
ROOT=$(repo_root "$0")

run_target() {
    command=$1
    shift
    case $command in
        criterion) run_script "$ROOT" check-criterion-performance-targets.sh "$@" ;;
        e2e) run_script "$ROOT" check-e2e-performance-targets.sh "$@" ;;
        soak) run_script "$ROOT" check-soak-targets.sh "$@" ;;
        coverage) run_script "$ROOT" check-coverage-targets.sh "$@" ;;
        regression) run_script "$ROOT" check-performance-regression.sh "$@" ;;
        *)
            usage_error 'usage: scripts/targets.sh <criterion|e2e|soak|coverage|regression> [args...]'
            ;;
    esac
}

[ "$#" -gt 0 ] || usage_error 'usage: scripts/targets.sh <criterion|e2e|soak|coverage|regression> [args...]'
command=$1
shift
run_target "$command" "$@"
