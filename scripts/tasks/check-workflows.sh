#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
workflow_dir="$root/.github/workflows"
ci="$workflow_dir/ci.yml"
fuzz="$workflow_dir/fuzz.yml"
performance="$workflow_dir/performance.yml"
release="$workflow_dir/release.yml"

fail() {
    printf 'workflow contract: %s\n' "$*" >&2
    exit 1
}

require_text() {
    file=$1
    value=$2
    grep -Fq -- "$value" "$file" \
        || fail "${file#"$root/"} is missing: $value"
}

reject_text() {
    file=$1
    value=$2
    if grep -Fq -- "$value" "$file"; then
        fail "${file#"$root/"} must not contain: $value"
    fi
}

[ -f "$ci" ] || fail '.github/workflows/ci.yml is missing'
[ -f "$fuzz" ] || fail '.github/workflows/fuzz.yml is missing'
[ -f "$performance" ] || fail '.github/workflows/performance.yml is missing'
[ -f "$release" ] || fail '.github/workflows/release.yml is missing'

actual=$(
    find "$workflow_dir" -maxdepth 1 -type f \
        \( -name '*.yml' -o -name '*.yaml' \) -print \
        | LC_ALL=C sort
)
expected=$(printf '%s\n%s\n%s\n%s' "$ci" "$fuzz" "$performance" "$release" | LC_ALL=C sort)
[ "$actual" = "$expected" ] \
    || fail 'workflow inventory must contain exactly ci.yml, fuzz.yml, performance.yml and release.yml'

tab=$(printf '\t')
for file in "$ci" "$fuzz" "$performance" "$release"; do
    if grep -n "$tab" "$file" >/dev/null; then
        fail "${file#"$root/"} contains a tab"
    fi
    reject_text "$file" 'continue-on-error:'
    reject_text "$file" '@main'
    reject_text "$file" '@master'
    reject_text "$file" '@latest'
done

if command -v ruby >/dev/null 2>&1; then
    ruby -e 'require "yaml"; ARGV.each { |path| YAML.parse_file(path) }' "$ci" "$fuzz" "$performance" "$release" \
        || fail 'YAML syntax validation failed'
fi

for value in \
    'name: CI' \
    'push:' \
    'pull_request:' \
    'workflow_dispatch:' \
    'contents: read' \
    'fail-fast: false' \
    'ubuntu-latest' \
    'macos-latest' \
    'windows-latest' \
    'actions/checkout@v6' \
    'actions/setup-node@v6' \
    'oven-sh/setup-bun@v2' \
    'bun-version: 1.3.11' \
    'dtolnay/rust-toolchain@stable' \
    'Swatinem/rust-cache@v2' \
    'components: rustfmt, clippy' \
    'cargo fmt --all -- --check' \
    'cargo clippy --workspace --all-targets --locked -- -D warnings' \
    'cargo check --workspace --all-targets --locked' \
    'cargo test --workspace --all-targets --no-fail-fast --locked' \
    'cargo build --release --workspace --locked' \
    './scripts/check.sh all' \
    "find scripts benches -type f -name '*.sh' -exec sh -n {} \\;" \
    'cargo check --manifest-path fuzz/Cargo.toml --bin parse_resolve --locked' \
    './scripts/verify.sh matrix' \
    './scripts/verify.sh actions'
do
    require_text "$ci" "$value"
done

for value in \
    './scripts/verify.sh package' \
    './scripts/verify.sh coverage' \
    './scripts/verify.sh perf' \
    './scripts/verify.sh soak' \
    'components: llvm-tools-preview' \
    'cargo install cargo-llvm-cov --version 0.6.21 --locked' \
    './scripts/verify.sh coverage-report' \
    'name: coverage-report' \
    'target/coverage' \
    'if-no-files-found: error'
do
    require_text "$ci" "$value"
done

for value in \
    'name: Rules fuzz' \
    'schedule:' \
    'cron: "17 3 * * *"' \
    'workflow_dispatch:' \
    'contents: read' \
    'group: rules-fuzz' \
    'cancel-in-progress: false' \
    'RSPROXY_FUZZ_RUNS: "0"' \
    'RSPROXY_FUZZ_SECONDS: "300"' \
    'RSPROXY_FUZZ_MAX_LEN: "65536"' \
    'actions/checkout@v6' \
    'dtolnay/rust-toolchain@nightly' \
    'Swatinem/rust-cache@v2' \
    'cargo install cargo-fuzz --version 0.13.2 --locked' \
    'cargo test -p rsproxy-rules --test fuzz_seeds --locked' \
    './scripts/verify.sh fuzz' \
    'if: failure()' \
    'actions/upload-artifact@v6' \
    'path: fuzz/artifacts/parse_resolve' \
    'if-no-files-found: ignore' \
    'retention-days: 14'
do
    require_text "$fuzz" "$value"
done

for value in \
    'name: Performance' \
    'pull_request:' \
    'schedule:' \
    'cron: "41 2 * * *"' \
    'workflow_dispatch:' \
    'contents: read' \
    'group: criterion-${{ github.ref }}' \
    'cancel-in-progress: true' \
    'fetch-depth: 0' \
    'benches/criterion/run.sh' \
    'scripts/targets.sh criterion' \
    'scripts/targets.sh regression' \
    'target/performance/criterion.json' \
    'actions/upload-artifact@v6' \
    'if-no-files-found: error' \
    'persist-credentials: false'
do
    require_text "$performance" "$value"
done

for value in \
    'name: Release' \
    'tags:' \
    'workflow_dispatch:' \
    'contents: read' \
    'fail-fast: false' \
    'ubuntu-24.04-arm' \
    'macos-15-intel' \
    'windows-11-arm' \
    'x86_64-unknown-linux-gnu' \
    'x86_64-unknown-linux-musl' \
    'aarch64-unknown-linux-gnu' \
    'aarch64-unknown-linux-musl' \
    'aarch64-apple-darwin' \
    'x86_64-apple-darwin' \
    'aarch64-pc-windows-msvc' \
    'x86_64-pc-windows-msvc' \
    'musl-tools' \
    'cargo build --release -p rsproxy --bin rsproxy --target ${{ matrix.target }} --locked' \
    './scripts/package-npm.sh native' \
    './scripts/package-npm.sh launchers' \
    'actions/upload-artifact@v6' \
    'actions/download-artifact@v6' \
    'if-no-files-found: error' \
    'id-token: write' \
    'registry-url: https://registry.npmjs.org' \
    'NODE_AUTH_TOKEN: ${{ secrets.NPM_TOKEN }}' \
    'npm publish' \
    '--access public --provenance' \
    'rsproxy-runtime' \
    'rsproxy-cli' \
    'rsproxy-bun' \
    'persist-credentials: false'
do
    require_text "$release" "$value"
done

for value in 'gh release' '*.tar.gz' 'contents: write'; do
    reject_text "$release" "$value"
done

printf 'CI, performance, fuzz, and npm/Bun release workflows satisfy the repository contract.\n'
