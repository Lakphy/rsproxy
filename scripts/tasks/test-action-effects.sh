#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$ROOT"

cargo test -p rsproxy-rules --test it corpus::
cargo test -p rsproxy-rules --test it whistle_migration::
cargo test -p rsproxy-rules --test it whistle_options::
cargo test -p rsproxy-engine --lib proxy::tests::action_effects::
