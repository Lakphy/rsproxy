# Testing

rsproxy uses layered tests and executable repository contracts. Prefer the
smallest relevant layer while iterating, then run the standard workspace gate
before opening a pull request.

## Standard local gate

With stable Rust 1.88 or later:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo test --workspace --all-targets --no-fail-fast --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
npm run check:packages
```

CI uses `cargo nextest` for the workspace test job, but ordinary `cargo test`
is the supported local fallback. Install `cargo-nextest` only when reproducing
that job exactly:

```sh
cargo install cargo-nextest --version 0.9.140 --locked
cargo nextest run --workspace --all-targets --no-fail-fast --locked
```

## Repository contracts

`xtask` keeps structural rules executable:

```sh
cargo xtask check lines
cargo xtask check layout
cargo xtask check typed-errors
cargo xtask check workflows
cargo xtask check api
cargo xtask check all
```

`check all` includes public-API snapshots and therefore requires the pinned
toolchain and checker used by CI:

```sh
rustup toolchain install nightly-2026-07-10 --profile minimal
cargo install cargo-public-api --version 0.52.0 --locked
```

When a reviewed public facade changes, regenerate snapshots with
`cargo xtask check api --bless` and commit the corresponding `api.txt` diff.
Do not bless unrelated drift.

The repository contracts enforce:

- the Rust source line limit configured in `xtask.toml`
- test placement and public integration-test directories
- typed domain errors in workspace and fuzz sources
- library public-API snapshots
- workflow inventory, syntax, permissions, triggers, action pins, and required
  commands
- the pinned Whistle fixture checksum and file inventory

## Focused Rust tests

Run a crate or named behavior while developing:

```sh
cargo test -p rsproxy-rules --test it corpus:: --locked
cargo test -p rsproxy-rules --test it whistle_migration:: --locked
cargo test -p rsproxy-net --test it --locked
cargo test -p rsproxy-engine --lib proxy::tests:: --locked
cargo test -p rsproxy-control --test public_api --locked
cargo test -p rsproxy-platform --test public_api --locked
cargo test -p rsproxy-trace --all-targets --locked
cargo test -p rsproxy-cli --test it --locked
cargo test -p xtask --all-targets --locked
```

Use `cargo test -p <package> -- --list` before relying on a narrow filter. A
misspelled filter can otherwise report success after running zero tests.

### Test boundaries

- `src/**/tests/` owns private unit and module-boundary behavior.
- `crates/<name>/tests/` owns public facade and executable integration tests.
- `rsproxy-engine` owns end-to-end proxy and rule-effect behavior.
- `rsproxy-net` owns protocol framing, transport, DNS, deadlines, and pool
  primitives without policy.
- `rsproxy-rules/tests/corpus/` is the executable public DSL contract. Anchored
  cases must remain synchronized with `docs/rules-dsl-spec.md`.
- `crates/rsproxy-rules/tests/fixtures/whistle-2.10.5/` is a pinned upstream
  compatibility fixture, not maintained documentation.

## Specialized verification scripts

`scripts/verify.sh` provides stable entry points around longer or
multi-package checks:

| Command | Purpose | Notes |
| --- | --- | --- |
| `./scripts/verify.sh actions` | Rules corpus, Whistle mappings/options, and real proxy action effects | Used by CI |
| `./scripts/verify.sh matrix` | Declared protocol-owner matrix across engine and net | Used by CI |
| `./scripts/verify.sh package` | npm/Bun target, manifest, pack, install, and launcher contracts | Requires Node and Bun for the full local run |
| `./scripts/verify.sh bench` | Small self-contained proxy benchmark plus JSON contract | Release build |
| `./scripts/verify.sh stream` | Explicit large-stream resource acceptance | Transfers 1 GiB by default; ignored by ordinary tests |
| `./scripts/verify.sh coverage-report` | Workspace and rules line-coverage thresholds | Requires `cargo-llvm-cov` |
| `./scripts/verify.sh fuzz` | Finite rules parser/resolver libFuzzer campaign | Requires nightly and `cargo-fuzz` |
| `./scripts/verify.sh all` | `actions`, `matrix`, `bench`, `package`, and `stream` | Does not include coverage or fuzzing |

The `all` command is intentionally expensive because it includes the 1 GiB
stream test. Use the individual commands during normal iteration.

## Coverage

CI enforces workspace line coverage of at least 85% and `rsproxy-rules` line
coverage of at least 95%:

```sh
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov --version 0.6.21 --locked
./scripts/verify.sh coverage-report
```

Reports are written below `target/coverage/`. Generated code, test-only code,
and platform-only branches are handled by the coverage script rather than by
ad hoc command flags in this document.

## Fuzzing

The versioned seed corpus is replayed by ordinary tests:

```sh
cargo test -p rsproxy-rules --test it fuzz_seeds:: --locked
```

Run a finite sanitizer-backed campaign with nightly Rust:

```sh
cargo install cargo-fuzz --version 0.13.2 --locked
RSPROXY_FUZZ_RUNS=1000 ./scripts/verify.sh fuzz
# or a time-bounded run
RSPROXY_FUZZ_RUNS=0 RSPROXY_FUZZ_SECONDS=300 ./scripts/verify.sh fuzz
```

`RSPROXY_FUZZ_MAX_LEN` must remain between 1 and 65536. The wrapper fuzzes a
temporary corpus copy so generated inputs do not modify checked-in seeds.

## Performance and soak tests

Criterion benchmarks and report validation:

```sh
./benches/criterion/run.sh target/performance/criterion.json
cargo xtask targets criterion target/performance/criterion.json
```

Other explicit drivers:

```sh
./benches/e2e/benchmark.sh
./benches/e2e/performance.sh
./benches/e2e/whistle.sh
./benches/soak/soak.sh
./scripts/verify.sh stream
```

These are environment-sensitive acceptance tools. Compare results produced on
equivalent hardware and settings; do not copy a local throughput number into a
long-lived correctness claim. The Whistle comparison uses the pinned
`whistle@2.10.5` lock and installs dependencies only below ignored `target/`
state unless `RSPROXY_WHISTLE_DIR` selects an existing matching installation.

## CI workflow map

`.github/workflows/ci.yml` runs on `main`, pull requests, merge queues, and
manual dispatch. Pull-request jobs wait on the `ci-approval` environment. Its
jobs cover:

- workspace checks/tests/release builds on Linux, macOS, and Windows
- the Rust 1.88 minimum supported version
- formatting, Clippy, and rustdoc warnings
- repository, public-API, shell, and fuzz-build contracts
- protocol, action, and npm distribution contracts
- cargo-deny supply-chain policy
- production line coverage

Two scheduled, manually dispatchable workflows run outside the pull-request
gate:

- `performance.yml`: absolute Criterion targets and regression comparison
- `fuzz.yml`: seed replay and a five-minute parser/resolver fuzz campaign

`release.yml` is documented in [Development and release process](release-process.md).
Workflow behavior is contract-checked, so update
`crates/xtask/src/check/workflow_contracts.rs` in the same change as an
intentional workflow edit.
