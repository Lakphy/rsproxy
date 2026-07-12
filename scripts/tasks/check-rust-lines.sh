#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
limit=${RSPROXY_RUST_LINE_LIMIT:-500}

violations=$(
    find "$root/crates" "$root/fuzz" \
        -path "$root/fuzz/target" -prune -o \
        -type f -name '*.rs' -exec wc -l {} + \
        | awk -v limit="$limit" '$2 != "total" && $1 > limit { print }'
)

if [ -n "$violations" ]; then
    printf 'Rust files over %s lines:\n%s\n' "$limit" "$violations" >&2
    exit 1
fi

printf 'All Rust files are at or below %s lines.\n' "$limit"
