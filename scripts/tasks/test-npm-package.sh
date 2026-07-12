#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/rsproxy-npm-contract.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM

fail() {
    printf 'npm/Bun package contract: %s\n' "$*" >&2
    exit 1
}

for command in node npm bun rustc tar; do
    command -v "$command" >/dev/null 2>&1 || fail "requires $command"
done

(cd "$root" && node --test packages/npm/tests/*.test.js)
(cd "$root" && bun test packages/npm/tests/*.test.js)

version=$(node -p "require('$root/package.json').version")
target=$(rustc -vV | sed -n 's/^host: //p')
case "$target" in
    aarch64-apple-darwin) native_name=rsproxy-darwin-arm64 ;;
    x86_64-apple-darwin) native_name=rsproxy-darwin-x64 ;;
    aarch64-unknown-linux-gnu) native_name=rsproxy-linux-arm64-gnu ;;
    aarch64-unknown-linux-musl) native_name=rsproxy-linux-arm64-musl ;;
    x86_64-unknown-linux-gnu) native_name=rsproxy-linux-x64-gnu ;;
    x86_64-unknown-linux-musl) native_name=rsproxy-linux-x64-musl ;;
    aarch64-pc-windows-msvc) native_name=rsproxy-win32-arm64-msvc ;;
    x86_64-pc-windows-msvc) native_name=rsproxy-win32-x64-msvc ;;
    *) fail "unsupported local Rust host $target" ;;
esac
native_package=${native_name#rsproxy-}

fixture_mode=generated
fixture=${RSPROXY_PACKAGE_TEST_BINARY:-}
if [ -n "$fixture" ]; then
    fixture_dir=$(CDPATH= cd -- "$(dirname -- "$fixture")" && pwd)
    fixture="$fixture_dir/$(basename -- "$fixture")"
    [ -x "$fixture" ] || fail "test binary is not executable: $fixture"
    expected_output=$("$fixture" --version)
    fixture_mode=provided
else
    fixture="$tmp/rsproxy"
    expected_output="rsproxy fixture $version"
    cat >"$fixture" <<EOF
#!/bin/sh
case "\${1:-}" in
    --version) printf 'rsproxy fixture $version\\n' ;;
    --exit-23) exit 23 ;;
    *) printf 'args:%s\\n' "\$*" ;;
esac
EOF
    chmod +x "$fixture"
fi

pack_set() {
    manager=$1
    dist="$tmp/$manager-packages"
    mkdir -p "$dist"
    "$root/scripts/package-npm.sh" native \
        --target "$target" --binary "$fixture" --manager "$manager" --dist "$dist" >/dev/null
    "$root/scripts/package-npm.sh" launchers \
        --manager "$manager" --dist "$dist" >/dev/null

    for name in "$native_name" rsproxy-runtime rsproxy-cli rsproxy-bun; do
        archive="$dist/$name-$version.tgz"
        [ -f "$archive" ] || fail "$manager did not create $(basename "$archive")"
        tar -tzf "$archive" | grep -Fx 'package/package.json' >/dev/null \
            || fail "$archive has no package manifest"
    done
    tar -tzf "$dist/$native_name-$version.tgz" \
        | grep -Fx 'package/bin/rsproxy' >/dev/null \
        || fail "$manager native package has no executable"
}

pack_set npm
pack_set bun

runtime_fixture="$tmp/runtime-fixture"
cp -R "$root/packages/npm/runtime" "$runtime_fixture"
node - "$runtime_fixture/package.json" "@rsproxy/$native_package" "$version" <<'NODE'
const fs = require('node:fs');
const [manifestPath, nativePackage, version] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
manifest.optionalDependencies = { [nativePackage]: version };
fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
NODE
mkdir -p "$tmp/npm-fixture" "$tmp/bun-fixture"
(cd "$runtime_fixture" && npm pack --silent --ignore-scripts \
    --pack-destination "$tmp/npm-fixture" >/dev/null)
(cd "$runtime_fixture" && bun pm pack --ignore-scripts --quiet \
    --destination "$tmp/bun-fixture" >/dev/null)

npm_dir="$tmp/npm-install"
mkdir -p "$npm_dir"
npm install --prefix "$npm_dir" --ignore-scripts --no-audit --no-fund \
    "$tmp/npm-packages/$native_name-$version.tgz" \
    "$tmp/npm-fixture/rsproxy-runtime-$version.tgz" \
    "$tmp/npm-packages/rsproxy-cli-$version.tgz" >/dev/null
npm_output=$("$npm_dir/node_modules/.bin/rsproxy" --version)
[ "$npm_output" = "$expected_output" ] \
    || fail "npm launcher returned unexpected output: $npm_output"
if [ "$fixture_mode" = generated ]; then
    if "$npm_dir/node_modules/.bin/rsproxy" --exit-23; then
        fail 'npm launcher did not forward the native exit status'
    else
        [ "$?" -eq 23 ] || fail 'npm launcher changed the native exit status'
    fi
fi

bun_dir="$tmp/bun-install"
mkdir -p "$bun_dir"
cat >"$bun_dir/package.json" <<EOF
{
  "private": true,
  "dependencies": {
    "@rsproxy/bun": "file:$tmp/bun-packages/rsproxy-bun-$version.tgz",
    "@rsproxy/runtime": "file:$tmp/bun-fixture/rsproxy-runtime-$version.tgz",
    "@rsproxy/$native_package": "file:$tmp/bun-packages/$native_name-$version.tgz"
  },
  "overrides": {
    "@rsproxy/runtime": "file:$tmp/bun-fixture/rsproxy-runtime-$version.tgz",
    "@rsproxy/$native_package": "file:$tmp/bun-packages/$native_name-$version.tgz"
  }
}
EOF
(cd "$bun_dir" && bun install --offline --ignore-scripts >/dev/null)
bun_output=$("$bun_dir/node_modules/.bin/rsproxy" --version)
[ "$bun_output" = "$expected_output" ] \
    || fail "Bun launcher returned unexpected output: $bun_output"
if [ "$fixture_mode" = generated ]; then
    if "$bun_dir/node_modules/.bin/rsproxy" --exit-23; then
        fail 'Bun launcher did not forward the native exit status'
    else
        [ "$?" -eq 23 ] || fail 'Bun launcher changed the native exit status'
    fi
fi

printf 'npm and Bun package, install, launcher, and native forwarding contracts passed.\n'
