#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$SCRIPT_DIR/lib.sh"
ROOT=$(repo_root "$0")

run_check() {
    case $1 in
        lines) run_script "$ROOT" check-rust-lines.sh ;;
        layout) run_script "$ROOT" check-test-layout.sh ;;
        whistle) run_script "$ROOT" check-whistle-isolation.sh ;;
        workflows) run_script "$ROOT" check-workflows.sh ;;
        *) usage_error 'usage: scripts/check.sh <lines|layout|whistle|workflows|all>' ;;
    esac
}

command=${1:-all}
case $command in
    all)
        for check in lines layout whistle workflows; do
            run_check "$check"
        done
        ;;
    *) run_check "$command" ;;
esac
