# npm and Bun distribution

This directory is the only public distribution boundary for rsproxy.

```text
targets.json       Rust target to npm package contract
runtime/           platform/libc resolver and native process forwarding
cli/               shared npm/Bun entry package published as @rsproxy/cli
scripts/           deterministic native and launcher package builder
tests/             mapping, version, manifest, and runtime contracts
```

The runtime chooses one of eight optional native packages. Supported targets
are macOS arm64/x64, Linux arm64/x64 with glibc or musl, and Windows arm64/x64
with MSVC. Native packages contain no JavaScript launcher and expose no extra
command; only `@rsproxy/cli` installs `rsproxy`.

The npm registry is the single artifact registry. npm and Bun users install the
same launcher package:

```sh
npm install --global @rsproxy/cli
# or
bun add --global @rsproxy/cli
```

The installed command keeps the standard Node 18+ shebang. Bun-only users can
execute the same registry artifact with `bunx --bun @rsproxy/cli`.

There are no lifecycle scripts and no install-time Rust compilation. Release
automation publishes native packages first, then `@rsproxy/runtime`, and the
shared launcher last. All package versions must exactly match the Cargo
workspace version. Every Cargo package sets `publish = false`, so crates.io is
not a fallback channel.

Publishing requires an authorized public `@rsproxy` npm scope and the repository
secret `NPM_TOKEN`. See the [release process](../../docs/release-process.md) for
the current credential, provenance, and recovery contracts.

Run the local contracts with:

```sh
./scripts/verify.sh package
```

The contract packages and installs a host fixture with both package managers.
Only the current host executable is run locally; other targets are checked as
manifest and mapping contracts until their release jobs run on native runners.
