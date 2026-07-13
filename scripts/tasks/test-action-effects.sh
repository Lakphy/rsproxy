#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

cargo test -p rsproxy-rules --test corpus
cargo test -p rsproxy-rules --test whistle_migration
cargo test -p rsproxy-rules --test whistle_options
cargo test -p rsproxy-engine --lib proxy::tests::action_effects::
