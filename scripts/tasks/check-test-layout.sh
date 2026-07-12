#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)

inline_modules=$(
    find "$root/crates" -type f -name '*.rs' \
        -exec grep -Hn -E '^[[:space:]]*(pub([[:space:]]*\([^)]*\))?[[:space:]]+)?mod[[:space:]]+tests[[:space:]]*\{' {} + \
        || true
)

if [ -n "$inline_modules" ]; then
    printf 'Inline test modules are not allowed; use src/<module>/tests.rs or tests/:\n%s\n' \
        "$inline_modules" >&2
    exit 1
fi

test_files=$(
    find "$root/crates" -type f -name '*.rs' \
        -exec grep -l -E '#\[[[:space:]]*(tokio::)?test([[:space:]]|\])' {} + \
        || true
)
misplaced_tests=''
if [ -n "$test_files" ]; then
    while IFS= read -r file; do
        relative=${file#"$root/"}
        case "$relative" in
            */tests.rs | */tests/*.rs) ;;
            *) misplaced_tests="${misplaced_tests}
${relative}" ;;
        esac
    done <<EOF
$test_files
EOF
fi

if [ -n "$misplaced_tests" ]; then
    printf 'Test functions outside a dedicated test path:%s\n' "$misplaced_tests" >&2
    exit 1
fi

missing_integration_dirs=''
for manifest in "$root"/crates/*/Cargo.toml; do
    crate_dir=${manifest%/Cargo.toml}
    if [ ! -d "$crate_dir/tests" ]; then
        missing_integration_dirs="${missing_integration_dirs}
${crate_dir#"$root/"}/tests"
    fi
done

if [ -n "$missing_integration_dirs" ]; then
    printf 'Crates missing a public integration-test directory:%s\n' \
        "$missing_integration_dirs" >&2
    exit 1
fi

printf 'All Rust tests use dedicated module or crate-level test paths.\n'
