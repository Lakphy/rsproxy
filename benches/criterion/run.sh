#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname "$0")/../.." && pwd)
OUTPUT=${1:-"$ROOT/target/performance/criterion.json"}

cd "$ROOT"
cargo bench -p rsproxy-rules --bench rules --locked -- --noplot
cargo bench -p rsproxy-trace --bench trace --locked -- --noplot
cargo bench -p rsproxy --features bench-support --bench certificates --locked -- --noplot
"$ROOT/benches/criterion/collect.sh" "$OUTPUT"
