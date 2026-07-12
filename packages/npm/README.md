# npm and Bun distribution

This directory is the only public distribution boundary for rsproxy.

```text
targets.json       Rust target to npm package contract
runtime/           platform/libc resolver and native process forwarding
cli/               Node launcher published as @rsproxy/cli
bun/               Bun launcher published as @rsproxy/bun
scripts/           deterministic native and launcher package builder
tests/             mapping, version, manifest, and runtime contracts
```

The runtime chooses one of eight optional native packages. Supported targets
are macOS arm64/x64, Linux arm64/x64 with glibc or musl, and Windows arm64/x64
with MSVC. Native packages contain no JavaScript launcher and expose no extra
command; only `@rsproxy/cli` and `@rsproxy/bun` install `rsproxy`.

The npm registry is the single artifact registry. npm users install the Node
launcher, while Bun users install the Bun launcher:

```sh
npm install --global @rsproxy/cli
bun add --global @rsproxy/bun
```

There are no lifecycle scripts and no install-time Rust compilation. Release
automation publishes native packages first, then `@rsproxy/runtime`, and the
two launchers last. All package versions must exactly match the Cargo workspace
version. Every Cargo package sets `publish = false`, so crates.io is not a
fallback channel.

Before the first tag release, create or authorize the public `@rsproxy` npm
scope and add an npm automation token as the repository secret `NPM_TOKEN`.

Run the local contracts with:

```sh
./scripts/verify.sh package
```

The contract packages and installs a host fixture with both package managers.
Only the current host executable is run locally; other targets are checked as
manifest and mapping contracts until their release jobs run on native runners.
