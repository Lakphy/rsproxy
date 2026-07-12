# Architecture

## Workspace boundaries

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

`rsproxy-cli` is the executable crate and composition root:

- `cli/`: command parsing and command adapters. `cli/help.rs` owns root and
  subcommand usage, intercepted before config/auth/process side effects;
  `cli/completions.rs` owns shell script generation. It may call application
  services but does not implement proxy protocol behavior.
- `cli/config.rs` selects configuration sources and applies CLI overrides;
  `cli/config/file.rs` owns the strongly typed TOML schema and file-value
  application.
- `logging.rs` is the process-observability boundary. It owns filter/format
  environment parsing and one stderr-only `tracing-subscriber`; request Trace
  storage remains entirely inside `rsproxy-trace`.
- `cli/rules/`: rule-command routing and storage fallback stay in the facade;
  group CRUD/edit/list, request metadata/API-query construction, and local
  benchmark execution live in separate `groups`, `request`, and `bench` modules.
- `rule_store/`: ordered group metadata and atomic file replacement are isolated
  from snapshot coordination. A single ArcSwap snapshot contains group text,
  enable state, order, and the compiled `RuleSet`; each request retains one
  snapshot through request planning and response-period evaluation.
- `cli/ca/`: certificate construction/fingerprints, CA filesystem state, and
  platform trust-store operations are isolated from CA command orchestration.
  `ca/trust/{linux,windows}.rs` separates p11-kit and current-user Root-store
  commands from the macOS security/keychain implementation.
- `cli/system_proxy/`: macOS networksetup, Linux gsettings and Windows
  registry/WinINet backends own their platform plans and execution. Linux and
  Windows preserve prior values and roll back partial mutation failures.
- `cli/daemon.rs`: synchronously binds both listeners before publishing
  readiness, supervises listener threads, and owns pidfile identity checks and
  cross-platform process termination. A failed start cleans up both child and
  pidfile.
- `app.rs`: runtime configuration and shared state. `app/mitm_failures.rs` owns
  the bounded, TTL-aware host memory used only by automatic MITM fallback.
- `proxy/`: downstream protocol handling, routing, transformations, TLS and
  forwarding. `proxy.rs` is the private module facade.
- `proxy/transforms/values.rs`: the single runtime boundary for inline,
  storage-value, and file-backed bytes/text. Text actions require UTF-8; byte
  actions preserve binary input. Request, response, URL, routing, and trace
  policy reuse this resolver. Mock references/raw payloads also use it; mock
  file candidate/directory lookup stays in `mock.rs` and feeds the shared text
  or binary renderer.
- `rsproxy-rules/action/delete.rs` owns the typed pathname and body-path model;
  `rsproxy-rules/parser/delete.rs` compiles Whistle-style property strings and
  escape/index syntax into that model. `proxy/transforms/delete.rs` orchestrates
  URL, request, response, and trailer phases, while
  `proxy/transforms/delete/body.rs` owns MIME-gated JSON/form/JSONP mutation.
  The data plane never reparses property names, and whole/nested body variants
  participate in the same bounded-body degradation policy.
- `http/`: HTTP/1 wire primitives. Request-head framing validation and
  request-body/trailer decoding are separate stateful modules. The body reader
  supports bounded collection with lossless overflow continuation; reader state,
  collection policy, and chunk-line/trailer validation live in separate
  `body`, `body/collect`, and `body/trailers` modules. Response parsing and
  writing stay independent.
- `proxy/server/`: listener admission and the plain HTTP/1 client loop. CONNECT
  orchestration, policy precedence, non-consuming protocol detection, reusable
  tunneled HTTP/1 handling, MITM TLS and request-input errors are isolated in
  `connect`, `connect_policy`, `probe`, `inner_http`, `mitm` and `request`.
- `proxy/http_flow/`: request orchestration with separate session setup,
  pending-body planning, and upstream completion/error attribution modules.
- `proxy/h2_bridge/`: bounded downstream HTTP/2 adaptation. `request` exposes
  DATA/trailers to the existing request-body planner without collecting the
  complete stream; `response` incrementally decodes the shared HTTP/1-shaped
  response output into an early head plus backpressured DATA/trailer frames.
- `proxy/request_stream.rs`: deadline-aware h1/h2 upload relay, bounded trace
  tee capture, byte accounting, h1 chunk re-encoding and h2 DATA/trailer
  forwarding.
- `proxy/request_util.rs`: shared `ThrottlePacer` keeps one monotonic pacing
  cursor across buffered writes and streaming frames. Request/response writers
  reuse it for the lifetime of one transfer; deadline-aware paths cannot sleep
  beyond the absolute request budget, and programmatic zero rates are normalized.
- `proxy/trace_helpers.rs`: shared trace lifecycle guards, lazy visible-session
  start, and the bounded request/response body-event emitter. HTTP/2 reuses
  `Bytes` slices; byte-slice transports copy only the configured preview prefix.
- `proxy/tunnel.rs`: plain/TLS bidirectional copy and tunnel byte observation.
  Passthrough payloads are never retained as previews; each direction emits only
  observed byte counts, and final snapshots correct any dropped queue events.
- `proxy/websocket/` and `proxy/websocket_frame.rs`: upgrade policy remains in
  the facade; nonblocking TLS/MITM forwarding, concurrent plain-TCP forwarding,
  frame decoding and trace projection are separate state machines.
- `proxy/connect_tls/`: upstream TLS handshake IO, TLS trace records and
  DNS/TCP/TTFB timing budgets. These modules share no state machine.
- `proxy/routing/`: selected routes own only the transport dial endpoint and
  proxy-hop shape. The parsed URL remains the origin identity for authority,
  TLS SNI and certificate verification. In particular, `host(...)` may route a
  named HTTPS origin to a literal address without changing the hostname passed
  to the upstream TLS handshake. Shared authority formatting brackets IPv6
  literals for request URLs, Host headers, dial addresses and route labels.
- `proxy/tls/`: rule-derived TLS policy, rustls/trust-root configuration and
  leaf-certificate file lifecycle.
- `proxy/upstream_response/`: protocol-neutral response policy with separate
  buffered and streaming downstream writers.
- `proxy/h1_forward/`: the single synchronous HTTP/1 implementation. Its pooled
  direct fast path and unpooled compatibility fallback share one ownership
  boundary; WebSocket upgrade completion lives separately in
  `proxy/websocket_forward.rs`.
- `upstream_h2/` and `upstream_message.rs`: Hyper HTTP/2 request conversion,
  keyed stream admission, connection/send/response lifecycle, bounded request
  frames and streaming dispatch. Callers use one `dispatch` outcome and an
  opaque connector; a stream lease remains active until request upload and
  response body have both ended.
- `transfer_timing.rs`: shared monotonic one-shot transfer timers and a Hyper
  `Body` wrapper. Request timers freeze when the body reaches EOF or is dropped;
  response timers are shared with the bounded pump and freeze at body/trailer
  completion or error.
- `upstream_body.rs`: bounded `Bytes` frame transport shared by both upstream
  protocols; timed channels expose response-receive duration without moving
  ownership out of the producer, and pool leases remain owned until completion.
- `control/`: TCP/Unix/Windows-named-pipe transport, authentication, request
  routing and resource handlers. `windows_pipe.rs` owns Win32 handles and the
  shared blocking `Read + Write` server/client adapter; named pipes retain token
  authentication and reject remote clients. Resource routes live under
  `control/routes/`; expected streaming client disconnects are debug events,
  while real request failures remain WARN.
- `http.rs`, `h2.rs`, `dns.rs`, `async_io.rs`: protocol and IO primitives shared
  by the data plane.
- `h2/`: downstream HTTP/2 admission, channel-backed response bodies,
  wire-message conversion, and shared Tokio/rustls runtime adaptation live in
  separate `server`, `body`, `message`, and `runtime` modules behind the thin
  `h2.rs` facade. The transport header-list setting is bounded to the configured
  application limit plus a fixed 64KiB diagnostic margin; service validation
  still enforces the exact limit and can therefore return an explanatory 431.
- `json/` and `tui/`: presentation adapters. TUI state, rendering and text/JSON
  formatting are independent modules.
- `examples/bench_origin.rs` and `examples/bench_client.rs` are standalone M0
  acceptance drivers, not production protocol modules. `benches/e2e/benchmark.sh`
  composes them with the release binary and a curl preflight; its versioned JSON
  output is the input boundary for later M5 performance gates.

## Dependency rules

Keep domain logic in the library crate that owns it. CLI and control handlers
translate inputs and outputs; they should not duplicate rule evaluation or trace
storage behavior. Protocol-specific pools must remain isolated so h1 and h2
admission, reuse and timeout semantics cannot accidentally share state.

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
./scripts/check.sh lines
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
`proxy/h2_bridge/tests/{request,response}.rs`,
`proxy/tunnel/tests.rs`,
`transfer_timing/tests.rs`,
`proxy/tests/h1_forward.rs` and
`upstream_h2/tests/{message,connection,pool,request_body,streaming,timeouts}.rs`.
Real TLS protocol crossings additionally live in
`proxy/tests/{h2_downstream_streaming,origin_h2_streaming}.rs`. See
[Testing](testing.md) for the complete map and commands.
Cross-protocol acceptance edges live under
`proxy/tests/protocol_matrix/{websocket,mtls,headers,names}.rs`; each starts real
listeners and drives the proxy path rather than calling codecs or transforms in
isolation.

Executable product contracts are also split by responsibility:
`tests/cli_daemon_lifecycle.rs` owns process recovery and identity safety,
`tests/cli_json_contracts.rs` owns machine-readable shapes,
`tests/cli_completions.rs` owns shell generation, and
`tests/cli_product_matrix/{offline,online}.rs` owns command-family workflows.

Trace collector white-box tests are split into `src/tests/collector.rs` for
queue/follow lifecycle and `src/tests/events.rs` for incremental assembly,
concurrent producers, out-of-order metadata, drop correction and body budgets.
`src/tests/spill_read.rs` owns append/clear/eviction races for collector-independent
spill snapshots.
The public event/follow contract remains in `rsproxy-trace/tests/`.
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
`scripts/verify.sh matrix` first verifies that all 34 exact test names
still exist, then runs each owner. This prevents a renamed or removed test from
turning an exact filter into a successful zero-test invocation.

Cross-layer value behavior lives in `proxy/tests/value_actions.rs`; the
seven-form field resolver matrix is split into `value_runtime_matrix.rs` and
`value_runtime_matrix/cases.rs`. The 17-test action-effect harness runs the real
`handle_client` path with local TCP/TLS fixtures and enforces an exact one-owner
partition of all 46 public families; `scripts/verify.sh actions` combines
it with the parser and migration contracts. Public DSL source classification, parse matrix,
Whistle migration contract, and bounded-complexity gate live in
`rsproxy-rules/tests/{value_sources,value_matrix,whistle_migration,complexity}.rs`;
these are deliberately separate from private parser unit tests.

Machine-readable Whistle registry and option classifications live under
`rsproxy-rules/tests/contracts/`, not the rules corpus directory. Their public
runners validate source drift against the immutable 2.10.5 evidence snapshot
under `tests/fixtures/` and execute every recipe declared implemented. The full
upstream Node repository is neither a workspace input nor a test dependency.

The layout is enforced by `scripts/check.sh layout`: inline `mod tests {}`
blocks are rejected, test functions must live under `tests/` or `tests.rs`, and
every crate must retain its public integration-test directory.

## Automation boundaries

`.github/workflows/ci.yml` keeps portable Rust check/test/release-build work in
an Ubuntu/macOS/Windows matrix. POSIX repository checks, formatting, Clippy,
fuzz-target compilation, quality-gate contracts and coverage run in dedicated
Ubuntu jobs. `.github/workflows/performance.yml` compares Criterion results for
the base and current commits on one runner and blocks regressions above 10%.
`.github/workflows/fuzz.yml` owns the daily nightly/libFuzzer run. The npm
distribution boundary lives under `packages/npm/`: `@rsproxy/runtime` resolves
one of eight OS/architecture/libc packages, while `@rsproxy/cli` and
`@rsproxy/bun` provide runtime-specific launchers. `release.yml` builds those
native packages and publishes only to the npm registry, with launchers published
after every native artifact. Other target mappings are structural contracts in
this round; current executable qualification remains local macOS ARM64.

`scripts/check.sh workflows` constrains workflow inventory, syntax, permissions,
action majors, triggers, platforms and commands. Clippy denies all default
warnings without a project-wide lint exception; request, response, connection
and timeout state cross module boundaries through named context structures.
