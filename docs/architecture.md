# Architecture

## Workspace governance

Workspace package metadata, third-party dependency versions, Rust and Clippy
lints, and release optimization policy are defined once in the root
`Cargo.toml`. Every member inherits Rust 1.88 as the minimum supported version
and the workspace lint policy. Release binaries use thin LTO, one codegen unit,
and stripped symbols; unwinding remains enabled because connection threads use
panic isolation as a reliability boundary.

The workspace denies `unwrap` in production code. A panic is reserved for an
already-broken process-local invariant, principally poisoned synchronization
state or a value whose shape was established immediately beforehand. Such sites
must use `expect` and name the violated invariant (for example, `trace collector
worker lock poisoned`); recoverable input, protocol, filesystem and network
failures continue through typed results. Tests may use `unwrap` and `expect` for
fixture ergonomics. This preserves the D-04 connection-thread panic boundary
while making every intentional panic searchable and useful in a crash report.

The Rust facade of each product library is snapshotted in
`crates/<crate>/api.txt`. `cargo xtask check api` regenerates those views with
the pinned `nightly-2026-07-10` rustdoc JSON toolchain and
`cargo-public-api 0.52.0`, then rejects additions, removals or signature/trait
changes. After reviewing an intentional change, use
`cargo xtask check api --bless`; the resulting snapshot diff must be explained
in the PR description, including why the affected cross-crate contract changes.
`cargo xtask check all` includes this check, while product builds remain on the
stable/MSRV toolchains.

Install the two review-only tools before running the API gate locally:

```sh
rustup toolchain install nightly-2026-07-10 --profile minimal
cargo install cargo-public-api --version 0.52.0 --locked
```

## rsproxy-rules

`rsproxy-rules` owns the rule language. Its public facade re-exports stable rule,
action, matcher and result types. Parsing is split into matcher, condition,
metadata, transform, TLS and syntax modules; matching, indexing, resolution and
human-readable explanations are separate execution stages. Request-body
planning inspects only candidate rules, so the proxy can decide whether a body
must be aggregated before resolution and can resolve body-independent behavior
when an oversized body must remain streaming.

The public integration corpus under `rsproxy-rules/tests/corpus/` is the
machine-readable language contract. Cases assert stable parse-error stages,
action families and group/line provenance; selected cases are bidirectionally
anchored from `docs/rules-dsl-spec.md`.

The matcher facade separates URL-pattern execution, action family/stacking
metadata, condition evaluation, and the public URL model into
`matcher/{pattern,action,condition,url}.rs`.

Action data is split by responsibility under `action/`: `value.rs` owns the
public `Value` source model and key grammar, `host_pool.rs` owns deterministic
round-robin selection, `replace_pattern.rs` owns compiled replacements, and
`template_validation.rs` walks action fields before publication. The parser
constructs structured values; it does not leave `@key` or `<path>` encoded in
plain strings.

## rsproxy-trace

`rsproxy-trace` owns captured session data and persistence. `model` contains the
public completed-session records and `event` defines the incremental producer
contract. `store` is a thin cloneable facade around atomic IDs and a bounded
command channel. `store/worker` is the single-owner collector;
`store/{pending,memory}` own in-flight assembly and completed-session eviction,
while `store/{config,counters,stats,follow}` own budget partitioning, queue
accounting, observability and subscriber APIs. A follow handle owns a strong
liveness token while the worker keeps only a weak reference, so stats can prune
closed subscribers without waiting for another session. `spill/read` captures open-file
snapshots with immutable data/index lengths; CRC verification, decompression and
body assembly run on the query caller after the collector resumes. Event submission uses
nonblocking `try_send`; control-plane reads use ordered commands as
synchronization barriers. `spill` owns segment lifecycle, and
`spill/{path,codec}` isolate naming/index and encoding/CRC logic. Serialization
is independent of storage policy.

## rsproxy-net

`rsproxy-net` is the protocol-and-IO leaf crate. It owns transport mechanisms
used by `rsproxy-engine`, but it does not own rule evaluation, trace storage,
control resources, CLI configuration or proxy policy. Its manifest has no
dependency on another rsproxy crate.

The `rsproxy-net` public facade is deliberately explicit and grouped as follows:

- readiness and async adaptation: `ReadyIo`, `AsyncIo`;
- DNS: `DnsConfig`, `DnsResolver`, `DnsStatsSnapshot`;
- HTTP/1 parsing, framing and writing: `RawRequest`, `RawResponseHead`,
  `RequestHead`, `RequestBodyFraming`, `RequestBodyReader`, `RequestBodyRead`,
  `BoundedRequestBody`, `read_request`, `read_request_head`,
  `read_request_head_tcp`, `read_request_body_bounded`, `read_response_head`,
  `read_response_head_buffered`, `validate_request_trailers`, `header`,
  `set_header`, `remove_header`, `reason_phrase`, `write_response`,
  `write_response_head`, `write_response_head_with_connection`, and
  `write_response_with_version_and_connection`;
- absolute request deadlines and the shared H2 runtime: `RequestDeadline`,
  `TimeoutBudget`, `is_request_total_timeout`, `h2_runtime`;
- bounded upstream response bodies and keyed admission: `UpstreamBody`,
  `UpstreamBodyFrame`, `CollectedBody`, `BoundedBody`, `ActivityStore`,
  `KeyedActivity`, `PoolWaitSpec`, `acquire_slot`;
- upstream H2 dispatch: `UpstreamH2Request`, `UpstreamH2Response`, `H2Body`,
  `H2Config`, `H2DispatchRequest`, `H2Outcome`, `H2Connector`, `H2Connected`,
  `StreamingH2Request`, `dispatch`;
- downstream H2 service adaptation: `DownstreamH2Config`,
  `DownstreamH2Request`, `DownstreamH2RequestFrame`, `DownstreamH2Response`,
  `DownstreamH2ResponseHead`, `DownstreamH2ResponseFrame`, and
  `serve_downstream_h2`.

The `test-support` feature additionally exports `read_request_body_all`,
`TestReceiveTimer` and `test_timed_upstream_body_channel`, and enables the
test-only `UpstreamBody::{channel,from_collected,collect}` helpers; none is part
of the normal production surface. The integration contract in
`rsproxy-net/tests/public_api.rs` compiles representative HTTP, DNS, deadline,
pool, async-adapter, upstream-body, typed upstream-H2-dispatch and injected
downstream-H2-handler uses against this facade.

The implementation remains private behind that facade:

- `http/{request,response}` separates request and response wire behavior;
  `request/{head,body}` separates head parsing from stateful body consumption,
  and `body/{collect,trailers}` owns bounded collection and trailer validation.
- `dns.rs` owns resolver construction, literal-address bypass, positive/negative
  cache policy and counters. `async_io.rs` adapts blocking-ready streams to
  Tokio IO, while `runtime.rs` owns the shared H2 runtime.
- `request_deadline.rs` owns absolute and stage budgets;
  `transfer_timing.rs` owns the internal one-shot Hyper body timer. The timer is
  intentionally not a facade type: body and H2 results expose the measurements.
- `upstream_body.rs` owns the bounded DATA/trailer channel and lossless overflow
  continuation; `upstream_pool.rs` owns protocol-neutral keyed capacity waits.
- `upstream_h2/{message,request_body,connection,pool,streaming}` separates wire
  conversion, request production, connection/send lifecycle, H2 session state,
  and buffered/streaming dispatch. Callers see outcomes and an opaque connector,
  not pool internals.
- `downstream_h2/{message,body,server}` separates wire validation/conversion,
  response-body frames and Hyper server admission. `serve_downstream_h2`
  accepts a cloneable generic async handler from its caller; the handler maps a
  `DownstreamH2Request` to a future `DownstreamH2Response`.
  `rsproxy-engine::proxy::h2_bridge` injects the policy/data-plane pipeline
  through that seam, so the leaf crate does not import engine or CLI state.

## rsproxy-engine

`rsproxy-engine` is the policy and proxy data-plane library. Its public facade
exports `ProxyConfig`, `SharedState`, `EngineHandle`, typed status/replay
snapshots, `RuleStore` and `serve`; callers construct runtime-owned state from a
data-plane-only configuration and pass a bound `TcpListener` to the listener
entry point. CLI arguments, control resources and presentation formats do not
enter this crate.

- `state.rs` owns `ProxyConfig`, `SharedState`, the DNS/trace/rule runtime
  assembly, upstream trust roots, the MITM certificate LRU, and the bounded
  TTL-aware automatic-MITM failure cache.
- `rule_store/` owns ordered group metadata, atomic file replacement, watching
  and ArcSwap snapshot publication. A single snapshot contains group text,
  enable state, order and the compiled `RuleSet`; each request retains one
  snapshot through request planning and response-period evaluation.
- `handle.rs` is the typed control boundary. `EngineHandle` exposes rules,
  immutable status snapshots and replay without revealing `SharedState` fields;
  replay execution remains engine-owned because it reuses data-plane request
  parsing and response limits.
- `proxy/` owns all downstream protocol handling, routing, transformations,
  TLS/MITM, forwarding, WebSocket, tunnel, mock and Trace integration.
  `proxy.rs` is its private module facade and `proxy/server.rs` supplies the
  public `serve` entry point.
- `proxy/transforms/values.rs` is the runtime boundary for inline,
  storage-value and file-backed bytes/text. Text actions require UTF-8; byte
  actions preserve binary input. Request, response, URL, routing, mock and Trace
  policy reuse this resolver.
- `proxy/transforms/delete.rs` orchestrates typed URL, request, response and
  trailer deletion, while `proxy/transforms/delete/body.rs` owns MIME-gated
  JSON/form/JSONP mutation. The data plane never reparses property names.
- `proxy/server/` separates plain HTTP/1 admission, CONNECT orchestration,
  policy precedence, non-consuming protocol detection, reusable tunneled HTTP,
  MITM TLS and request-input errors.
- `proxy/http_flow/` separates session setup, pending-body planning and upstream
  completion/error attribution. `proxy/h2_bridge/` adapts bounded downstream H2
  frames to the same policy pipeline and emits incremental response head,
  backpressured DATA and trailers.
- `proxy/request_stream.rs`, `proxy/request_util.rs` and
  `proxy/trace_helpers.rs` own deadline-aware h1/h2 relay, shared transfer
  pacing, byte accounting, bounded Trace capture and lifecycle guards.
- `proxy/tunnel.rs`, `proxy/websocket/` and `proxy/websocket_frame.rs` own
  direction-aware tunnel counts, plain/TLS duplex forwarding, frame decoding
  and Trace projection without retaining opaque passthrough payloads.
- `proxy/connect_tls/`, `proxy/routing/` and `proxy/tls/` keep dial routing,
  origin identity/SNI, handshake timing, policy, trust roots and leaf
  certificate lifecycle separate. `host(...)` may change a dial endpoint
  without changing the origin hostname used for TLS verification.
- `proxy/h1_forward/` is the single synchronous HTTP/1 implementation;
  `proxy/upstream_response/` owns protocol-neutral buffered/streaming response
  completion, and `proxy/websocket_forward.rs` owns upgrade completion.
- White-box rule-store, state and complete proxy data-plane tests live beside
  these modules. `tests/public_api.rs` compiles the engine facade from outside
  the crate. `benches/certificates.rs`, enabled by `bench-support`, measures the
  engine-owned MITM certificate issue, disk-cache and server-config-cache paths.
- `examples/bench_origin.rs` and `examples/bench_client.rs` are engine-owned M0
  acceptance drivers. `benches/e2e/benchmark.sh` composes them with the release
  binary and a curl preflight.

## rsproxy-control

`rsproxy-control` owns the complete control-plane protocol boundary. Its public
facade exposes the control client/auth vocabulary plus `ControlOptions`,
`ControlState`, `ControlListener`, `bind` and `serve`:

- `server.rs` owns listener selection and lifecycle. `ControlOptions` contains
  control endpoint and display metadata, while `ControlState` composes those
  options with an `EngineHandle` and the engine's `TraceStore` handle; it does
  not receive `SharedState` or CLI `AppConfig`.
- `server/{router,routes,query,values}.rs` owns authenticated HTTP/1 control
  dispatch, resource handlers and query/value parsing. Status, rules and replay
  call the typed `EngineHandle::{status_snapshot,rules,replay}` boundary;
  sessions and follow use the cloned Trace handle. Its small blocking HTTP/1
  request/response wire is intentionally local to the control protocol and the
  crate does not depend on `rsproxy-net`.
- `client.rs` owns request/NDJSON-follow transport, while `client/auth.rs` and
  `server/auth.rs` own token files, generation, validation and constant-work
  bearer/header authentication. The CLI retains only configuration-source
  precedence and passes the resolved token to this client facade.
- `server/windows_pipe.rs` owns the Windows named-pipe listener/stream adapter;
  Unix-domain sockets and TCP listeners share the same router. The Windows pipe
  and TCP transports require token authentication, while a private 0600 Unix
  socket uses local peer/file permissions.
- `shapes.rs` and `shapes/har.rs` own stable JSON, table and HAR projection used
  by the API. The 34 unit tests remain beside the error/server/client/shape
  modules, and `tests/public_api.rs` contains three black-box facade contracts.

## rsproxy-platform

`rsproxy-platform` is the operating-system integration leaf. Its manifest has
only third-party dependencies and its public surface is grouped by capability:

- `ca` owns root-CA generation and fingerprinting, typed root/leaf storage
  paths and status, PEM reads, persisted leaf material, and macOS/Linux/Windows
  trust-store install/uninstall outcomes. `TrustOptions` selects dry-run and
  keychain inputs without introducing CLI presentation types.
- `process` owns typed PID parsing, daemon detachment, liveness and termination,
  plus `unix_control_socket_path`, including the deterministic short-path
  fallback for long storage directories. Listener/readiness orchestration stays
  in the CLI.
- `system_proxy` owns `ProxyPlatform`, `ProxyAction`, `ProxyTarget`,
  `ProxyOptions`, the render-neutral `ProxyPlan`/`ProxyOutcome`, and the paired
  `plan_system_proxy`/`execute_system_proxy` entrypoints. The CLI renders a dry
  run from the typed plan; normal execution applies the same platform-specific
  plan with rollback where supported and returns typed status/change data.

Root-CA lifecycle and storage therefore do not enter the data plane. The
cryptographic MITM hot path remains in `rsproxy-engine`:
the CLI composition root reads initialized root PEM through the platform
facade and injects a redacted `CaMaterial` value into `ProxyConfig` once at
startup. The engine neither knows the root file names nor reads root material
from `storage`; it uses the injected in-memory material for MITM signing and
upstream trust, while `storage` is only the leaf-certificate cache location.
`issue_leaf_certificate` also signs an explicitly requested leaf from supplied
PEM. Neither leaf crate imports the other. The workspace-level unsafe deny
remains in force elsewhere; `rsproxy-platform` has a documented crate-level
allowance only for narrowly localized Unix/Windows process calls and Windows
WinINet notification calls in `process.rs` and `system_proxy/windows.rs`.

Eight white-box unit tests cover native trust and system-proxy behavior. Ten
public-only CA/error/process tests live in the crate-level integration targets,
and five `tests/public_api.rs` contracts compile typed errors, root
lifecycle/trust, process operations, system-proxy plan/execution and Unix
socket-path assembly without CLI or engine types.

## rsproxy-cli

`rsproxy-cli` is the executable package and only composition root. Its
user-facing `[[bin]]` remains named `rsproxy`, while Rust code imports the
library as `rsproxy_cli`:

- `app.rs` wraps engine `ProxyConfig` in CLI-owned `AppConfig`, retaining only
  listener/control metadata and configuration-precedence state. It projects a
  `ControlOptions` value for the control crate and explicitly injects platform
  root-CA PEM into the engine configuration; `ControlState` itself is owned and
  assembled by `rsproxy-control` from an `EngineHandle`.
- `cli/command.rs` owns the typed `clap` derive tree: the root command, shared
  client/runtime argument groups and completion shells stay in the facade, while
  `command/{rules,trace,ca,proxy}.rs` hold the per-family subcommand structs
  mirroring their handler modules. Clap renders help/version before config,
  authentication or process side effects, and `clap_complete` generates the
  four shell completion formats from that same tree. `cli/config.rs` and `cli/config/file.rs` merge
  typed CLI overrides over TOML and defaults. Commands call
  engine/control/platform services but do not implement proxy protocol or
  operating-system behavior.
- `cli/rules/` retains command routing, API-query construction and offline
  storage fallback, using the engine-owned `RuleStore` facade.
- `logging.rs` is the process-observability boundary: it owns filter/format
  parsing and the stderr-only `tracing-subscriber`; request Trace storage stays
  in the engine through `rsproxy-trace`.
- `tui/` remains a CLI presentation adapter over the public control client.
  Control routing, JSON/HAR shapes, token-auth mechanics and Windows named-pipe
  transport no longer live in the CLI.
- `cli/ca.rs` and `cli/system_proxy.rs` are argument/config/result adapters over
  the typed platform API. The CA adapter calls engine-owned
  `issue_leaf_certificate` only for explicit leaf issuance and delegates root
  lifecycle, persistence and trust to platform.
- `cli/daemon.rs` retains daemon/listener/readiness orchestration, but delegates
  PID parsing, detachment, liveness, termination and Unix control-socket path
  assembly to `rsproxy-platform::process`.

Each library boundary exposes a domain result instead of a string error
channel: `NetError`, `EngineError`, `ControlError`, `PlatformError`, and
`RuleModelError`. Cross-crate variants retain their source with `#[from]` or an
explicit source field; contextual I/O variants likewise preserve the original
`io::Error`. The CLI aggregates these as `CliError`. `main.rs` is the only
rendering boundary: after successful clap parsing it uses the typed global
`json` field, maps runtime failures to stable additive
`rsproxy.cli.error/v1` codes, and exits with 1. Usage failures are rendered by
clap and exit with 2; daemon state conflicts exit with 3. Raw argv inspection
is confined to clap's parse-failure path, where no typed command exists yet and
`--json` still determines the error representation.

## xtask

`xtask` is the eighth workspace member and the typed repository-automation
boundary. Its public facade exposes typed release, check and report-validation
entry points; its binary maps `cargo xtask release`, `check` and `targets` onto
those APIs without embedding policy in shell dispatchers.

- `release/` validates semantic versions, derives the eight native optional
  dependencies from `packages/npm/targets.json`, and transactionally
  synchronizes Cargo lockfiles, the root distribution manifest and the
  runtime/npm/Bun manifests. `--check` is read-only and is used by tag
  validation. npm packaging obtains the same version from `cargo metadata`, so
  Cargo package metadata is the only version authority; native platform
  manifests remain generated staging files.
- `check/` owns the Rust-line, test-layout, Whistle-fixture, typed-error and
  workflow contracts. `targets/` uses typed serde reports for coverage,
  Criterion, e2e, soak and regression thresholds.
- Shell remains responsible only for process orchestration such as local
  network fixtures, resource sampling, packaging and fuzz driver setup.
  `tests/public_api.rs` compiles the typed release/check/targets facade from
  outside the crate.

## Dependency rules

The current internal dependency graph is
`rsproxy-cli -> {rsproxy-control, rsproxy-engine, rsproxy-platform,
rsproxy-rules, rsproxy-trace}`,
`rsproxy-control -> {rsproxy-engine, rsproxy-rules, rsproxy-trace}`,
`rsproxy-engine -> {rsproxy-net, rsproxy-rules, rsproxy-trace}`, and
`rsproxy-trace -> rsproxy-rules`. `rsproxy-net`, `rsproxy-platform` and
`rsproxy-rules` have no internal dependency and must remain leaves.
`rsproxy-net` receives downstream H2 behavior through handler injection rather
than reaching upward into the engine or composition root; platform operations
are selected and rendered only by the CLI composition root.

Keep domain logic in the library crate that owns it. `rsproxy-engine` may depend
on protocol mechanisms, rule types and Trace storage, but it must not know CLI
arguments, control routes or presentation formats. CLI and control handlers
translate inputs and outputs; they should not duplicate rule evaluation or
Trace storage behavior. Protocol-specific pools must remain isolated so h1 and
h2 admission, reuse and timeout semantics cannot accidentally share state.

Prefer a thin module facade with explicit re-exports. Use `pub(crate)` or a
scoped visibility such as `pub(in crate::proxy)` for internal collaboration;
do not widen implementation types into the public crate API merely to make a
split compile.

Rule updates must compile and persist the complete ordered group set before one
ArcSwap publication. Request and response phases of an in-flight exchange must
share the same snapshot; control-plane reads may load the latest snapshot.

Create a submodule when a file combines independent state machines, persistence
formats, transports, presentation layers or resource families. Do not split a
single cohesive flow only to reduce line count.

## Size invariant

Every Rust source file must remain at or below 500 lines. The invariant covers
production and test code under `crates/` and is enforced by:

```sh
cargo xtask check lines
```

Treat 500 as a hard ceiling, not a target. Split by responsibility before a file
reaches the ceiling. Files above 400 lines should be reviewed for a natural
responsibility boundary before more code is added; exceptions require an
explicit rationale in review.

## Test boundaries

White-box unit tests live next to the implementation under
`src/<module>/tests/`. Public black-box tests live in each crate's conventional
`tests/` directory. Large suites use behavior-named children such as
`response_actions/{content,framing,headers}.rs`,
`routing/{single_hop,chains}.rs`,
`request_streaming/{fixed,rules,chunked}.rs`, and
`action_effects/{request,response,local_routing,control_flow,tls}.rs`,
`rsproxy-engine/src/proxy/h2_bridge/tests/{request,response}.rs`,
`rsproxy-engine/src/proxy/tunnel/tests.rs`,
`rsproxy-engine/src/proxy/tests/h1_forward.rs` and
`rsproxy-net/src/{http,downstream_h2}/tests/`,
`rsproxy-net/src/transfer_timing/tests.rs`, and
`rsproxy-net/src/upstream_h2/tests/{message,connection,pool,request_body,streaming,timeouts}.rs`.
Real TLS protocol crossings additionally live in
`rsproxy-engine/src/proxy/tests/{h2_downstream_streaming,origin_h2_streaming}.rs`. See
[Testing](testing.md) for the complete map and commands.
Cross-protocol acceptance edges live under
`rsproxy-engine/src/proxy/tests/protocol_matrix/{websocket,mtls,headers,names}.rs`;
each starts real listeners and drives the proxy path rather than calling codecs
or transforms in isolation.

Executable product contracts are also split by responsibility:
`tests/cli_daemon_lifecycle.rs` owns process recovery and identity safety,
`tests/it/cli_json_contracts.rs` owns machine-readable shapes,
`tests/it/cli_completions.rs` owns shell generation, and
`tests/cli_product_matrix/{offline,online}.rs` owns command-family workflows.

Trace collector white-box tests are split into `src/tests/collector.rs` for
queue/follow lifecycle and `src/tests/events.rs` for incremental assembly,
concurrent producers, out-of-order metadata, drop correction and body budgets.
`src/tests/spill_read.rs` owns append/clear/eviction races for collector-independent
spill snapshots.
The public event/follow contract remains in `rsproxy-trace/tests/`.
The `rsproxy-net/tests/public_api.rs` black-box contract covers only the public
facade, including generic downstream-H2 handler injection.
The `rsproxy-engine/tests/public_api.rs` contract constructs `ProxyConfig` and
`SharedState`, reaches `EngineHandle`/`RuleStore` through the facade, and
type-checks `serve`. The three `rsproxy-control/tests/public_api.rs` contracts
compose a control state from `ControlOptions` plus `EngineHandle` and compile
the client authentication/endpoint vocabulary without private module access;
34 control unit tests cover typed errors, the server, client, local wire and
shape internals.
The five `rsproxy-platform/tests/public_api.rs` contracts cover the typed error,
CA/trust, process, system-proxy and Unix socket-path facade; eight platform unit
tests remain beside private native implementations.
Phase 6 applied the public-only D-15 test criterion without widening any API:
35 tests moved into crate integration targets across
`rsproxy-net/tests/it/{dns,errors,http_buffered_head,http_tcp_head,request_deadline}.rs`,
`rsproxy-engine/tests/it/{errors,rule_store}.rs`, and
`rsproxy-platform/tests/it/{ca,errors,process}.rs`. Protocol/H2/body/timing,
watcher, system-proxy and native trust suites still need private or
`test-support` constructors and deliberately remain white-box tests.
Across `rsproxy-net`, `rsproxy-control`, `rsproxy-rules`, `rsproxy-engine`,
`rsproxy-platform`, `rsproxy-trace` and `xtask`, the seven
`tests/public_api.rs` targets contain 27 facade-contract tests
(7 + 3 + 2 + 5 + 5 + 3 + 2). The CLI has executable black-box contracts
instead of a library facade snapshot.
Loop 96 additionally exercised live follow, HTTP/tunnel timing, zstd spill
snapshots, JSON/HAR export, TUI and replay through a release daemon, CLI and curl.
Release-process resource acceptance lives in
`rsproxy-cli/tests/large_stream_resource.rs` and is invoked explicitly through
`scripts/verify.sh stream`, so the ordinary workspace suite does
not transfer 1GiB on every edit.
The local proxy macrobenchmark is likewise explicit: `scripts/verify.sh bench`
validates the `rsproxy-benchmark/v1` result without turning every `cargo test`
run into a nested release build.
The cross-protocol owner matrix is explicit for the same reason:
`scripts/verify.sh matrix` loads the inventories of both `rsproxy-engine` and
`rsproxy-net`, first verifies that all 34 exact test names exist in their
declared packages, then runs each owner. This prevents a renamed, moved or
removed test from turning an exact filter into a successful zero-test
invocation.

Cross-layer value behavior lives in
`rsproxy-engine/src/proxy/tests/value_actions.rs`; the seven-form field resolver
matrix is split into `value_runtime_matrix.rs` and
`value_runtime_matrix/cases.rs`. The 17-test action-effect harness runs the real
engine listener path with local TCP/TLS fixtures and enforces an exact one-owner
partition of all 46 public families; `scripts/verify.sh actions` combines it
with the parser and migration contracts. Public DSL source classification, parse matrix,
Whistle migration contract, and bounded-complexity gate live in
`rsproxy-rules/tests/it/{value_sources,value_matrix,whistle_migration,complexity}.rs`;
these are deliberately separate from private parser unit tests.

Machine-readable Whistle registry and option classifications live under
`rsproxy-rules/tests/contracts/`, not the rules corpus directory. Their public
runners validate source drift against the immutable 2.10.5 evidence snapshot
under `tests/fixtures/` and execute every recipe declared implemented. The full
upstream Node repository is neither a workspace input nor a test dependency.

The layout is enforced by `cargo xtask check layout`: inline `mod tests {}`
blocks are rejected, test functions must live under `tests/` or `tests.rs`, and
every crate must retain its public integration-test directory. The same check
verifies the pinned Whistle snapshot, file inventory, SHA-256 hashes and driver
version. `cargo xtask check typed-errors` parses Rust with `syn` and rejects
`Result<_, String>` and `Result<_, &'static str>` throughout `crates/` and
`fuzz/`, without source exemptions.

## Automation boundaries

`.github/workflows/ci.yml` runs once per change — pushes trigger it only on
`main`, pull requests and merge groups cover branches, and a concurrency group
cancels superseded PR runs. It keeps portable Rust check/test/release-build
work in an Ubuntu/macOS/Windows matrix and checks the workspace separately on
the declared Rust 1.88 MSRV. The matrix runs filesystem, source and workflow
checks; the Ubuntu repository-contract job runs `cargo xtask check all`,
including the pinned-nightly API snapshot gate, so one checked-in snapshot is
not compared against three host-specific `cfg` surfaces. Formatting, Clippy
and rustdoc, distribution contracts, fuzz-target compilation and coverage run
in dedicated parallel Ubuntu jobs. A cargo-deny job enforces advisory,
license, ban and registry-source policy, and `.github/dependabot.yml` opens
grouped cargo, npm and github-actions update PRs through the same gates.
`.github/workflows/performance.yml` compares Criterion
results for the parent and current commits on one runner and blocks
regressions above 10% through typed `cargo xtask targets` report parsing.
`.github/workflows/fuzz.yml` owns the nightly libFuzzer run; it and
`performance.yml` run on daily schedules plus manual dispatch, decoupled from
pushes and PRs, and are expected to pass before every release. The npm
distribution boundary lives under `packages/npm/`: `@rsproxy/runtime` resolves
one of eight OS/architecture/libc packages, while `@rsproxy/cli` and
`@rsproxy/bun` provide runtime-specific launchers. `release.yml` builds those
native packages, publishes to the npm registry with launchers published after
every native artifact, and then — only after npm publishing succeeds — creates
a GitHub release with one binary archive per target, a `SHA256SUMS` manifest
and changelog-derived notes; `contents: write` is granted solely to that final
job. Other target mappings are structural contracts in this round; current
executable qualification remains local macOS ARM64. The operating procedure
for development and releases is `docs/release-process.md`.

`cargo xtask check workflows` constrains workflow inventory, YAML syntax,
permissions, stable action references, triggers, platforms and commands.
`scripts/` retains only process orchestration for coverage, fuzzing, packaging,
network/resource tests and benchmarks; repository policy and JSON threshold
logic live in tested Rust. Clippy denies all default warnings without a
project-wide lint exception; request, response, connection and timeout state
cross module boundaries through named context structures.
