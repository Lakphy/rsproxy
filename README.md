# rsproxy

rsproxy is a Rust workspace for a programmable HTTP/HTTPS debugging proxy. The
workspace is split by domain rather than by deployment unit:

```text
crates/
  rsproxy-cli/    binary, proxy data plane, control API, CLI and TUI
  rsproxy-rules/  rule DSL plus pinned test-only Whistle evidence fixtures
  rsproxy-trace/  session model, in-memory store and spill persistence
packages/npm/     npm/Bun launchers, native-target map and package contracts
docs/             live design docs plus archived qualification evidence
benches/e2e/      reproducible local proxy benchmark orchestration
scripts/          repository checks
.github/workflows/ optional cross-platform compatibility CI and scheduled rules fuzzing
```

The dependency direction is `rsproxy-cli -> rsproxy-trace -> rsproxy-rules` and
`rsproxy-cli -> rsproxy-rules`. The two library crates expose thin `lib.rs`
facades; implementation details stay in responsibility-named modules.

Install the CLI through one of the two supported package managers. Both use the
npm registry; the Bun package has its own Bun shebang and does not require Node
at runtime.

```sh
npm install --global @rsproxy/cli
bun add --global @rsproxy/bun
```

The distribution map covers macOS, Linux and Windows on arm64/x64, including
both glibc and musl Linux. Only the current Apple M1 Pro macOS ARM64 package is
executed in this local qualification round; the other package/target mappings
are present but are not claimed as target-OS runtime verification.

Build and test the workspace:

```sh
cargo fmt --all -- --check
cargo build --workspace --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/check.sh all
npm run check:packages
./scripts/verify.sh package
./scripts/verify.sh all
./scripts/targets.sh criterion target/performance/criterion.json
```

Lifecycle and control commands use a storage-scoped local endpoint by default:
Unix uses a private domain socket (with a deterministic short-path fallback),
while Windows uses an authenticated named pipe. TCP remains available through
`--api HOST:PORT` and requires the generated or configured API token.

```sh
rsproxy start --storage ~/.rsproxy
rsproxy status --storage ~/.rsproxy --json
rsproxy completions zsh
rsproxy stop --storage ~/.rsproxy
```

Query commands support machine-readable JSON. A failed command invoked with
`--json` writes one `rsproxy.cli.error/v1` document to stderr.

Foreground process logs use `tracing` and always go to stderr. Set
`RSPROXY_LOG` (or `RUST_LOG`) to select a filter and
`RSPROXY_LOG_FORMAT=text|json` to select the output contract. Request/session
Trace remains a separate bounded data product exposed by the control API.

`benches/e2e/benchmark.sh` is the small release smoke. Formal M5 drivers live in
`benches/criterion/`, `benches/e2e/performance.sh`,
`benches/e2e/whistle.sh`, and `benches/soak/`. Their versioned JSON reports are
checked through `scripts/targets.sh`; coverage is collected by
`scripts/verify.sh coverage-report` with workspace/rules thresholds of 85%/95%.
The Whistle comparison uses the lock under `benches/e2e/whistle-driver/` and
installs its pinned dependency only into ignored `target/bench-deps/` state.

The release workflow builds eight native packages and publishes only to the npm
registry: `@rsproxy/cli` for npm and `@rsproxy/bun` for Bun. It does not publish
Cargo crates, standalone GitHub release archives, Homebrew formulae, or other
installer formats. The package contract is `scripts/verify.sh package`; current
runtime qualification remains local macOS ARM64.

See [Architecture](docs/architecture.md), [Configuration](docs/configuration.md),
[Testing](docs/testing.md), and the [technical design](docs/rsproxy-tech-design.md)
before changing cross-module behavior. Historical qualification records live in
`docs/archive/` and are not part of the active design surface.
