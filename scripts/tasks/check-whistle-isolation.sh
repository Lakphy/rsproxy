#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
FIXTURE="$ROOT/crates/rsproxy-rules/tests/fixtures/whistle-2.10.5"
DRIVER="$ROOT/benches/e2e/whistle-driver"

fail() {
    printf 'whistle isolation: %s\n' "$*" >&2
    exit 1
}

[ ! -e "$ROOT/whistle" ] || fail 'root whistle/ checkout must not exist'
[ -f "$FIXTURE/SNAPSHOT.toml" ] || fail 'snapshot metadata is missing'
[ -f "$FIXTURE/SHA256SUMS" ] || fail 'snapshot hashes are missing'
[ -f "$FIXTURE/LICENSE" ] || fail 'upstream license is missing'

evidence_files=$(find "$FIXTURE/docs" "$FIXTURE/lib" "$FIXTURE/test" -type f \
    | wc -l | tr -d ' ')
[ "$evidence_files" = 75 ] \
    || fail "expected 75 evidence files, found $evidence_files"
(
    cd "$FIXTURE"
    shasum -a 256 -c SHA256SUMS >/dev/null
) || fail 'snapshot hash verification failed'

[ "$(jq -r '.dependencies.whistle // empty' "$DRIVER/package.json")" = 2.10.5 ] \
    || fail 'driver package must pin whistle 2.10.5'
[ "$(jq -r '.packages["node_modules/whistle"].version // empty' \
    "$DRIVER/package-lock.json")" = 2.10.5 ] \
    || fail 'driver lock must resolve whistle 2.10.5'

if rg -n --glob '!check-whistle-isolation.sh' \
    '\$ROOT/whistle|root\.join\("whistle/' \
    "$ROOT/crates" "$ROOT/benches" "$ROOT/scripts" >/dev/null
then
    fail 'active code still references a root whistle/ checkout'
fi

printf 'whistle isolation: ok (75 pinned evidence files, benchmark package 2.10.5)\n'
