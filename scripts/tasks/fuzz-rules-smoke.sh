#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
runs=${RSPROXY_FUZZ_RUNS:-1000}
seconds=${RSPROXY_FUZZ_SECONDS:-0}
max_len=${RSPROXY_FUZZ_MAX_LEN:-65536}

for value in "$runs" "$seconds" "$max_len"; do
    case $value in
        ''|*[!0-9]*)
            echo "fuzz limits must be non-negative integers" >&2
            exit 2
            ;;
    esac
done
if [ "$runs" -eq 0 ] && [ "$seconds" -eq 0 ]; then
    echo "RSPROXY_FUZZ_RUNS or RSPROXY_FUZZ_SECONDS must be positive" >&2
    exit 2
fi
if [ "$max_len" -eq 0 ] || [ "$max_len" -gt 65536 ]; then
    echo "RSPROXY_FUZZ_MAX_LEN must be between 1 and 65536" >&2
    exit 2
fi

corpus=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-fuzz-corpus.XXXXXX")
trap 'rm -rf "$corpus"' EXIT HUP INT TERM

cp "$root"/fuzz/corpus/parse_resolve/* "$corpus"/
cd "$root"
if [ "$seconds" -gt 0 ]; then
    cargo +nightly fuzz run parse_resolve "$corpus" -- \
        "-max_total_time=$seconds" "-max_len=$max_len"
else
    cargo +nightly fuzz run parse_resolve "$corpus" -- \
        "-runs=$runs" "-max_len=$max_len"
fi
