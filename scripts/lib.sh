#!/bin/sh

repo_root() {
    CDPATH= cd -- "$(dirname -- "$1")/.." && pwd
}

run_script() {
    root=$1
    script=$2
    shift 2
    "$root/scripts/tasks/$script" "$@"
}

usage_error() {
    printf '%s\n' "$1" >&2
    exit 2
}
