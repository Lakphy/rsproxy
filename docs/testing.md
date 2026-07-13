# Testing

rsproxy uses the two standard Rust test layers. A virtual Cargo workspace does
not use one repository-level Rust test directory; each crate owns its tests:

- Unit tests that need private implementation details live beside their module under
  `crates/<crate>/src/<module>/tests/`.
- Public, black-box integration tests live in each crate's conventional
  `crates/<crate>/tests/` directory.

Cargo compiles `src/.../tests/` through `#[cfg(test)]`, so those tests can verify
private parsers, pools and protocol state machines. Files under a crate-level
`tests/` directory are compiled as separate crates and can only use the public
API. Keeping both layers is intentional; moving every test into one top-level
directory would either expose internals or lose focused unit coverage.

Pure black-box suites within a crate are modules of one `tests/it/main.rs`
harness, which preserves public-only compilation while paying the linker cost
once. `public_api.rs` remains a separate facade smoke contract. CLI daemon,
product-matrix and large-stream targets remain isolated because their process,
port and resource lifecycles rely on binary-level separation.

```text
crates/rsproxy-cli/
  src/cli/command.rs                 clap derive 命令树、typed 参数与多值等价性
  src/cli/tests/                     CLI 参数、配置与命令适配器白盒测试
  tests/                             executable/public black-box tests
    it/                               轻量 CLI 黑盒测试的单一链接 harness
      main.rs                         轻量 suites 的 Cargo integration target 入口
      cli_help.rs                     所有 command/subcommand 帮助快速退出且无副作用
      cli_completions.rs              Bash/Zsh/Fish/PowerShell 生成与错误合同
      cli_json_contracts.rs           查询 JSON shape 与单文档错误 schema
      cli_logging.rs                  stderr NDJSON 启动/监听事件与端口 0 黑盒合同
      cli_rule_groups.rs              离线规则组生命周期合同
      cli_trace_follow.rs             live NDJSON follow、heartbeat 与 count 合同
    cli_daemon_lifecycle.rs           真实 daemon 启停/重启/恢复/bind/PID 身份矩阵
    cli_daemon_lifecycle/             daemon target 的进程测试支持
    cli_product_matrix.rs             values/CA/proxy/trace/replay/TUI 产品路径入口
    cli_product_matrix/               product-matrix target 的 offline/online/support 模块
    large_stream_resource.rs         release 代理 1GiB、RSS 与 trace 资源验收（显式运行）
    large_stream_resource/           large-stream target 的进程测试支持
crates/rsproxy-control/
  src/client/tests.rs                请求、follow 与 token 发现/持久化客户端合同
  src/server/tests/                  auth、query、routes、resources 与断连分类
  src/shapes/tests/mod.rs            JSON/table/HAR shape 与敏感字段清理合同
  tests/public_api.rs                ControlOptions/ControlState 与 client facade 合同
crates/rsproxy-platform/
  src/ca/trust/macos/tests.rs        macOS security 子进程与错误分类
  src/system_proxy/tests.rs          macOS/Linux/Windows plan、rollback 与输出合同
  tests/it/{ca,errors,process}.rs     单一 harness 中的公开 CA/error/PID/path 合同
  tests/public_api.rs                CA/trust、process、proxy 与 Unix path 的 5 项 facade 合同
crates/rsproxy-engine/
  src/state/tests/                   运行期缓存容量、LRU 与 TTL 测试
  src/rule_store/watch/tests.rs      文件事件、debounce、失败恢复与退出
  src/proxy/tests/                   proxy network and policy unit/integration tests
    action_effects/                  46-family 真实 TCP/TLS 代理网络效果矩阵
    connect_modes.rs                 CONNECT 模式、探测与 pinning 重试生命周期
    connect_proxy.rs                 HTTP CONNECT/SOCKS5 地址、认证和错误矩阵
    h2_downstream_streaming.rs       TLS+h2 双向有界流式与 trailers
    origin_h2_streaming.rs           h1/h2 客户端到 TLS+h2 origin 大上传
    protocol_matrix/                 WS、mTLS、header、IPv6/punycode 真实网络边界
    websocket_nonblocking.rs         非阻塞双向 tunnel 的 backpressure/错误状态
    request_streaming/               fixed, chunked/trailer and rule-limit behavior
    response_actions/                content, framing and header behaviors
    routing/                         single-hop and proxy-chain planning
    timeouts/                        pool, request and setup deadlines
    value_actions.rs                 @key/file/template/capture runtime behavior
    value_runtime_matrix/            40 字段七类运行时解析矩阵
  src/proxy/request_util/tests.rs     跨写入 throttle pacing 与 deadline 边界
  src/proxy/transforms/delete/tests.rs typed delete URL/Content-Type 边界
  src/proxy/transforms/delete/body/tests.rs JSON/form/JSONP nested path 删除边界
  src/proxy/server/tests/            CONNECT policy/protocol probe unit tests
  src/proxy/h2_bridge/tests/         h2 请求适配与响应 framing 状态机
  src/proxy/tunnel/tests.rs          tunnel 双向字节事件与无 payload preview 合同
  src/proxy/tests/h1_forward.rs      pooled HTTP/1 framing, reuse and trace behavior
  tests/it/{errors,rule_store}.rs     单一 harness 中的 typed error 与 RuleStore 合同
  tests/public_api.rs                engine facade 的 handle/状态/规则/serve 公开合同
  benches/certificates.rs            MITM 证书签发/磁盘缓存/内存配置缓存基准
  examples/
    bench_origin.rs                  固定 1KiB keep-alive 本地 origin
    bench_client.rs                  并发持久连接、CL/chunked 响应 benchmark client
crates/rsproxy-net/
  src/http/tests/mod.rs              私有/test-support request/body/trailer/writer 边界
  src/downstream_h2/tests/mod.rs     下游 h2 message/body/server 与 handler 边界
  src/transfer_timing/tests.rs       单调传输计时器的 EOF/drop/冻结语义
  src/upstream_body/tests.rs         bounded frame collection and continuation
  src/upstream_h2/tests/             message, request body, streaming, pool and timeout behaviors
  tests/it/{dns,errors}.rs           单一 harness 中的 resolver/cache 与 typed error 合同
  tests/it/http_{buffered,tcp}_head.rs  HTTP head 快/缓冲路径与 framing 边界
  tests/it/request_deadline.rs       公开总 deadline 与 stage budget 归因
  tests/public_api.rs                net facade 的公开黑盒编译与行为合同
crates/rsproxy-rules/
  src/tests/                          private parser/resolver unit tests
    body_planning.rs                  candidate body-dependency planning
    conditions/                      request-period and response-period conditions
  tests/corpus/                       86 个 matcher/condition/action/composition/error case
  tests/it/                            纯 CPU 公开合同的单一链接 harness
    corpus.rs                          runner 与 37 个 DSL-spec 双向锚点
    properties.rs                      生成式合法/近似合法/任意输入不变量
    fuzz_seeds.rs                      cargo test 中回放版本化 fuzz seeds
    complexity.rs                      64KB hostile/scaling 时间预算门禁
    value_sources.rs                   公开 Value 语法与 key 边界
    value_matrix.rs                    40 个结构化值字段的四来源矩阵
    whistle_migration.rs               Whistle 源注册表与 action 映射 runner
    whistle_options.rs                 Whistle option 分类与实现配方 runner
  tests/contracts/                     非 corpus 的迁移/option TOML 合同
    whistle_migration.toml             whistle 96-name 源注册表与 46-family 映射
    whistle_options.toml               56 enable/66 disable/16 delete 分类
  tests/fixtures/whistle-2.10.5/       75 个只读上游证据、MIT 许可与 SHA-256
  tests/support/fuzz_harness.rs        seed test 与 libFuzzer 共用 harness
  tests/public_api.rs                  public RuleSet API tests
crates/rsproxy-trace/
  src/tests/collector.rs              队列溢出、查询屏障、内存预算、并发与关闭
  src/tests/events.rs                 增量事件、乱序/并发、丢弃校正与 pending 预算
  src/tests/spill_read.rs             collector 外读取、append/clear/eviction 快照竞态
  src/tests/mod.rs                    spill 轮转、恢复、压缩与损坏记录
  tests/public_api.rs                  public TraceStore API tests
crates/xtask/
  src/check/tests/                  lines/layout/Whistle/typed-error/workflow 正反合同
  src/release/tests.rs                版本同步、targets 派生、只读 check 与事务前置校验
  src/targets/tests/                coverage/criterion/e2e/soak/regression 阈值合同
  src/tests.rs                        非 semver CLI 输入退出码合同
  tests/public_api.rs                 release/check/targets typed facade 编译合同
fuzz/
  fuzz_targets/parse_resolve.rs       parse/resolve nightly sanitizer target
  corpus/parse_resolve/               valid/invalid 可审阅 seeds
benches/e2e/benchmark.sh              release 代理 + curl + Rust client 宏基准
benches/e2e/performance.sh            oha 吞吐、延迟、RSS 版本化报告
benches/e2e/whistle.sh                同机 Whistle pureProxy 严格对比
benches/e2e/whistle-driver/           固定 2.10.5 的独立 npm lock
benches/soak/soak.sh                  参数化 90m/QPS/规则/trace 稳态驱动
benches/criterion/                    rules/trace/engine certificate 基准编排与报告收集
packages/npm/tests/                   npm/Bun 平台映射、版本和 manifest 合同
scripts/verify.sh package             本机 npm/Bun pack/install/launcher 黑盒
scripts/verify.sh coverage-report                   llvm-cov 生产代码覆盖率门禁
scripts/verify.sh actions        action corpus、迁移和网络效果统一验收
scripts/verify.sh matrix    engine/net 双 package 的 34 项精确协议 owner 与防漂移验收
scripts/verify.sh bench             benchmark JSON 合同验收
.github/workflows/ci.yml              Ubuntu/macOS/Windows workspace 与 Ubuntu 合同门禁
.github/workflows/fuzz.yml            Ubuntu nightly 每日 sanitizer fuzz
.github/workflows/performance.yml     同 runner base/current Criterion 回归
.github/workflows/release.yml         八个原生 npm 包与两种启动器 tag 发布
cargo xtask check workflows           workflow inventory、语法和必跑命令静态合同
```

Phase 6 moved 10 public-only suites (35 tests) from module-private locations
into the net, engine and platform crate-level integration targets shown above,
without widening a facade. Across net, control, rules, engine, platform, trace
and xtask, the seven `tests/public_api.rs` targets contain 27 tests
(7 + 3 + 2 + 5 + 5 + 3 + 2). The CLI is covered through executable black-box
targets instead of a separate facade snapshot.

The larger suites are grouped by behavior rather than by implementation function:

- `rsproxy-cli/src/cli/tests/`: token precedence adapters, CA, runtime options, rule request
  construction, TOML precedence/error handling and system-proxy command plans.
- `rsproxy-control/src/{client,server,shapes,error}/tests/`: 34 unit tests for control
  authentication, token persistence/discovery, request/follow clients, query
  decoding, JSON/HAR shapes and resource routes, including ordered rule-group
  lifecycle and typed status/rules/replay calls through `EngineHandle`. The
  control protocol keeps its small blocking HTTP/1 wire local and does not
  depend on `rsproxy-net`.
- `rsproxy-control/tests/public_api.rs`: three black-box contracts compose
  `ControlOptions`/`ControlState` with an engine handle and exercise the stable
  client-auth and Unix/Windows endpoint vocabulary.
- `rsproxy-platform/src/{ca,system_proxy}/**/tests.rs`: eight private/native unit
  tests cover trust command handling and cross-platform system-proxy typed
  plan/outcome and rollback behavior. Ten public-only CA/error/process tests
  live under `rsproxy-platform/tests/`.
- `rsproxy-platform/tests/public_api.rs`: five black-box contracts exercise the
  typed error, CA/trust, process, system-proxy plan/execution and Unix
  socket-path facade without importing CLI or engine implementation types.
- `rsproxy-engine/src/rule_store/watch/tests.rs`: atomic disk reload, bounded event
  queue, debounce, invalid-edit rollback, recovery and worker shutdown.
- `rsproxy-engine/src/state/tests/`: bounded MITM certificate/failure cache
  capacity, LRU recency and TTL behavior.
- `rsproxy-engine/src/proxy/tests/`: connection/auth, routing, TLS policy, WebSocket,
  response actions, bounded request/response streaming, fixed and chunked upload
  fidelity, body-rule overflow behavior, HTTP/2 and TLS, staged timeouts, mock
  and trace behavior. `value_actions.rs` additionally drives references/files
  through request, response, URL, routing, mock, body, and trace execution;
  `value_runtime_matrix/` runs all 40 slots through seven source/error forms.
  `action_effects/` uses 17 real-network tests and assigns every
  `Action::FAMILIES` member to exactly one
  executable owner and observes its effect through real proxy/origin/client
  sockets, including routing, control flow, streaming and TLS ClientHello paths.
  `connect_modes.rs` drives real local sockets through global no-MITM,
  plaintext-HTTP detection, failed MITM memory, retry passthrough and strict mode.
- `rsproxy-engine/src/proxy/h2_bridge/tests/`: bounded request-channel adaptation,
  incremental response framing/trailers, incomplete bodies, and body-forbidden
  HEAD/204 behavior.
- `rsproxy-engine/src/proxy/tests/h2_downstream_streaming.rs`: one real TLS+h2
  client connection proves that an oversized upload reaches the origin before
  the client finishes sending and that response head/DATA arrive before the
  origin completes, while preserving both request and response trailers.
- `rsproxy-engine/src/proxy/tests/origin_h2_streaming.rs`: real h1 and h2 clients
  prove that oversized uploads reach a TLS/ALPN h2 origin before client
  completion, with body-rule degradation, trace prefixes, exact byte counts and
  request trailers preserved.
- `rsproxy-engine/src/proxy/server/tests/`: deterministic policy precedence and
  non-consuming TLS/HTTP/unknown/timeout protocol detection.
- `rsproxy-engine/src/proxy/tests/connect_modes.rs` additionally verifies that a
  passthrough tunnel remains pending while copy is open, completes exact duplex
  byte totals, handles refusal and MITM timeout without orphan events, and never
  starts a trace for `hide`.
- `rsproxy-engine/src/proxy/tunnel/tests.rs`: verifies direction-aware byte events
  aggregate without retaining opaque tunnel payloads.
- `rsproxy-engine/src/proxy/tests/h1_forward.rs`: pooled HTTP/1 connection reuse,
  framing errors, SSE, close-delimited bodies and trace fidelity.
- `rsproxy-net/src/http/tests/mod.rs`: private/test-support HTTP/1 request body,
  framing, trailer, writer and header-limit behavior. Public buffered and TCP
  head-reader behavior lives in
  `rsproxy-net/tests/it/{http_buffered_head,http_tcp_head}.rs`.
- `rsproxy-net/tests/it/dns.rs`: resolver configuration, cache behavior,
  literal-address bypass and DNS statistics through the public facade.
- `rsproxy-net/src/downstream_h2/tests/mod.rs`: downstream H2 wire conversion,
  bounded request/response frames, validation and generic handler service
  behavior. Engine-side `proxy/h2_bridge/tests/` owns policy-pipeline adaptation
  above this transport seam.
- `rsproxy-net/src/upstream_h2/tests/`: wire conversion, real pooled gRPC
  transport, bounded request-body error/deadline behavior, cold and pool-hit
  streaming uploads, connector/stream admission and timeout scopes.
- `rsproxy-net/src/transfer_timing/tests.rs` and the engine h1/h2 proxy tests verify
  one-shot timer freezing, EOF/drop behavior, independent slow upload/response
  intervals, and known-versus-unknown timing boundaries on transfer failures.
- `rsproxy-net/tests/public_api.rs`: compiles representative public HTTP, DNS,
  async IO, deadline, keyed-pool, upstream-body, typed upstream-H2 dispatch and
  injected downstream-H2 handler usage without private module access.
- `rsproxy-engine/tests/public_api.rs`: constructs public `ProxyConfig` and
  `SharedState`, reaches `EngineHandle` and the engine-owned `RuleStore`, and
  type-checks the listener `serve` entry point without private module access.
- `rsproxy-rules/src/tests/`: actions grouped by behavior, body-dependency
  planning, conditions, indexing and regular expressions.
- `rsproxy-rules/tests/it/corpus.rs`: runs 86 public cases and requires all 37
  specification anchors to resolve bidirectionally. Edge cases cover malformed
  authority/exact URL input, path/query glob boundaries, condition parameter
  validation, and response-dependent negation before/after a response snapshot.
- `rsproxy-rules/tests/it/properties.rs`: 256-case generated valid-rule reparse,
  structured near-valid failures and bounded arbitrary-input API traversal.
- `rsproxy-rules/tests/it/fuzz_seeds.rs`: replays the exact seed corpus used by the
  `parse_resolve` libFuzzer target through their shared harness.
- `rsproxy-rules/tests/it/value_matrix.rs`: parses 40 structured value slots with
  inline, template/capture, `@key`, and `<file>` sources (160 combinations).
- `rsproxy-rules/tests/it/value_sources.rs`: verifies public AST classification,
  quoted-literal behavior, key length/character boundaries, and parse errors.
- `rsproxy-rules/tests/it/whistle_migration.rs`: requires 46 source-backed supported
  mappings to cover exactly `Action::FAMILIES`, parses Whistle's 74 canonical
  protocols and 22 explicit aliases from the pinned 2.10.5 evidence fixture,
  and requires every name to be supported or explicitly deferred/removed.
- `rsproxy-rules/tests/it/whistle_options.rs`: extracts 56 `enable`, 66 `disable`,
  and 16 `delete` option classes from the same immutable fixture, requires an
  exact classification, parses/resolves every recipe marked implemented, and
  checks every `process-config` reference against real CLI help. Milestone-scoped
  deferred labels are rejected; out-of-v1 behavior must remain explicit v2.
- `rsproxy-rules/tests/it/complexity.rs`: exercises valid, malformed, many-rule,
  and fancy-regex inputs at the 64KB fuzz limit under finite time/scaling budgets.
- `rsproxy-trace/src/tests/collector.rs`: deterministically blocks the collector
  to verify nonblocking queue overflow, ordered query barriers, capacity-aware
  memory accounting, concurrent ID assignment, bounded live followers,
  immediate liveness-token cleanup in stats, and final-handle spill flushing.
- `rsproxy-trace/src/tests/events.rs`: verifies incremental assembly with body
  events arriving before metadata, concurrent duplex producers, queue-byte
  accounting, pending-session budgets, abort/orphan counters, and authoritative
  final snapshots after a chunk is dropped.
- `rsproxy-trace/src/tests/spill_read.rs`: pauses export after its immutable
  file-handle snapshot, proves record/stats continue through the collector, and
  verifies captured windows survive later append, `clear`, and budget eviction.
  A generation token prevents stale corruption reports from undoing clear.
- `rsproxy-trace/src/tests/mod.rs`: spill rotation, recovery, compression and
  corruption handling through the actor-backed public store facade.
- `rsproxy-cli/tests/it/cli_rule_groups.rs`: executable-level offline group
  set/list/disable/enable/test/remove lifecycle.
- `rsproxy-cli/tests/it/cli_trace_follow.rs`: executable-level live NDJSON follow,
  heartbeat handling and `--count` termination against a fake control API.
- `rsproxy-cli/tests/it/cli_logging.rs`: starts the real executable with ephemeral
  proxy/control ports, parses stderr NDJSON, and verifies stable startup,
  trust-root and bound-address events. It also prevents process logs from
  contaminating stdout command contracts.
- `rsproxy-cli/tests/it/cli_help.rs`: runs root, lifecycle, API, rules, values,
  trace, TUI, replay, CA, system-proxy and completion help for every supported
  subcommand through the real executable with a watchdog; help must succeed
  before runtime side effects, while unknown commands retain nonzero errors.
- `rsproxy-cli/tests/cli_daemon_lifecycle.rs`: starts detached real processes and
  verifies status, duplicate start, restart with rule retention, normal stop,
  abnormal-kill recovery, malformed pidfiles, occupied listener cleanup,
  ephemeral-port rejection and refusal to kill an unrelated live PID. Its
  Windows-only case uses the authenticated named-pipe transport.
- `rsproxy-cli/tests/it/cli_json_contracts.rs`: verifies exact query object keys and
  scalar shapes for rules, values, CA, status, trace and system-proxy plans;
  unknown, missing, unavailable and broken-config failures each emit one
  `rsproxy.cli.error/v1` document on stderr.
- `rsproxy-cli/tests/cli_product_matrix/`: splits offline values/CA/proxy and
  online trace/replay/TUI product paths into responsibility-named files behind
  one integration-test entry point.
- `rsproxy-cli/tests/it/cli_completions.rs`: validates Bash, Zsh, Fish and
  PowerShell scripts plus unsupported-shell errors without touching storage.
- `rsproxy-cli/tests/large_stream_resource.rs`: ignored-by-default release
  acceptance test that streams 1GiB from a real TCP origin through a spawned
  proxy, samples RSS, decodes the downstream chunked body, and verifies exact
  trace bytes, bounded preview, queue drops and total memory budget.
- `crates/*/tests/`: other public API and executable integration tests that
  cannot use private implementation details.

Run one layer or behavior group while iterating:

```sh
cargo test -p rsproxy-rules --lib tests::actions::
cargo test -p rsproxy-rules --lib tests::body_planning::
cargo test -p rsproxy-rules --test it corpus::
cargo test -p rsproxy-rules --test it complexity::
cargo test -p rsproxy-rules --test it properties::
cargo test -p rsproxy-rules --test it fuzz_seeds::
cargo test -p rsproxy-rules --test it value_matrix::
cargo test -p rsproxy-rules --test it value_sources::
cargo test -p rsproxy-rules --test it whistle_migration::
cargo test -p rsproxy-rules --test it whistle_options::
cargo test -p rsproxy-rules --test public_api
cargo test -p rsproxy-net --lib http::tests::
cargo test -p rsproxy-net --lib downstream_h2::tests::
cargo test -p rsproxy-net --lib upstream_h2::tests::
cargo test -p rsproxy-net --lib transfer_timing::tests::
cargo test -p rsproxy-net --test it dns::
cargo test -p rsproxy-net --test it http_buffered_head::
cargo test -p rsproxy-net --test it http_tcp_head::
cargo test -p rsproxy-net --test it request_deadline::
cargo test -p rsproxy-net --test public_api
cargo test -p rsproxy-control --lib
cargo test -p rsproxy-control --test public_api
cargo test -p rsproxy-platform --lib
cargo test -p rsproxy-platform --test it
cargo test -p rsproxy-platform --test public_api
cargo test -p rsproxy-engine --lib rule_store::watch::tests::
cargo test -p rsproxy-engine --lib proxy::tests::request_streaming::
cargo test -p rsproxy-engine --lib proxy::tests::connect_modes::
cargo test -p rsproxy-engine --lib proxy::tunnel::tests::
cargo test -p rsproxy-engine --lib proxy::h2_bridge::tests::
cargo test -p rsproxy-engine --lib proxy::tests::h2_downstream_streaming::
cargo test -p rsproxy-engine --lib proxy::tests::origin_h2_streaming::
cargo test -p rsproxy-engine --lib proxy::server::probe::
cargo test -p rsproxy-engine --lib proxy::tests::timeouts::
cargo test -p rsproxy-engine --lib proxy::tests::value_actions::
cargo test -p rsproxy-engine --lib proxy::tests::value_runtime_matrix::
cargo test -p rsproxy-engine --lib proxy::tests::action_effects::
cargo test -p rsproxy-engine --lib proxy::request_util::tests::
cargo test -p rsproxy-engine --test it
cargo test -p rsproxy-engine --test public_api
cargo test -p rsproxy-trace --all-targets
cargo test -p xtask --all-targets
```

Run the complete M1 action contract through one entry point:

```sh
./scripts/verify.sh actions
```

It runs the rules action corpus, Whistle migration/source-registry contract,
the option-level contract, and all 46-family real-network effect tests.

Run the protocol owner matrix through one entry point:

```sh
./scripts/verify.sh matrix
```

The script loads the exact test inventory from both `rsproxy-engine` and
`rsproxy-net`, then runs each of the 34 recorded owners in its declared package;
a renamed, deleted or package-mismatched test fails instead of reporting a
successful zero-test filter. Engine owns end-to-end proxy/policy crossings,
while net owns HTTP framing/header-limit and upstream-H2 message boundaries. Together
they cover h1 persistence/pipeline/Expect/auth, CONNECT
MITM/passthrough/probing, h2 bridge directions and bounded duplex flow,
request/response trailers, framing and body limits, gRPC, SSE, WebSocket frame
behavior, TLS/mTLS policy, and h1/h2 header-limit parsers. Dedicated
real-network owners additionally
exercise server-first WebSocket plus bidirectional frames and trace, required
upstream mTLS success/failure, 200KB and over-limit h1/h2 requests with explicit
431 responses, and IPv6 literal plus punycode host routing.

Run the explicit 1GiB resource acceptance test with a release proxy:

```sh
./scripts/verify.sh stream
```

The default contract transfers 1,073,741,824 bytes and permits at most 96MiB
RSS growth. `RSPROXY_LARGE_STREAM_BYTES` and
`RSPROXY_LARGE_STREAM_MAX_RSS_GROWTH_MB` are available for local diagnostics;
the recorded M3 acceptance uses the defaults.

Run the self-contained M0 proxy benchmark and its JSON contract check:

```sh
./benches/e2e/benchmark.sh
./scripts/verify.sh bench
```

The benchmark builds a release origin, proxy and persistent h1 client, performs
a 1KiB curl smoke through the proxy, then emits one `rsproxy-benchmark/v1` JSON
object. `RSPROXY_BENCH_REQUESTS`, `RSPROXY_BENCH_CONCURRENCY`, and
`RSPROXY_BENCH_SKIP_BUILD=1` control local runs. The contract test defaults to
128 requests at concurrency 8 and requires exact bytes with zero status or IO
errors. It proves the M0 script is runnable; it does not replace the
Criterion/oha M5 performance thresholds.

Run the complete workspace suite:

```sh
cargo test --workspace --all-targets --no-fail-fast --locked
```

The pre-restructure 2026-07-12 M5 baseline was 445 regular passing tests plus
the explicit 1GiB resource test ignored by default. The latest explicit run transferred
1,073,741,824 bytes in 679ms with 3,008KiB RSS growth and exact trace bytes.

The v1 qualification host is the current Apple M1 Pro macOS ARM64 machine. Its
formal proxy baseline is 45,392 requests/s at concurrency 16, with a 10%
regression floor of 40,853 requests/s; use
`RSPROXY_PERF_MIN_RPS=40853` for the local absolute-target check. The latest
10-second stability smoke completed 5,001/5,001 requests at 500 QPS with 1,001
rules, bounded RSS/FD growth, and zero pending, incomplete, orphaned, dropped,
or spill-error trace state. The steady-state qualification then ran for 6,307
seconds and covered 6,379,936 sessions across 106 minute samples. RSS ended
below its start with a negative last-half slope, FD peaked at 136 against a 144
limit, and Trace remained fully drained and lossless. Linux/Windows target-OS
execution is not part of the current local qualification. Their npm package
mappings and native-runner release jobs are defined, but this document does not
treat an unexecuted target as verified.

Run the production coverage, benchmark and stability gates explicitly:

```sh
./scripts/verify.sh coverage-report
./benches/criterion/run.sh target/performance/criterion.json
cargo xtask targets criterion target/performance/criterion.json
./benches/e2e/performance.sh
./benches/e2e/whistle.sh
./benches/soak/soak.sh
./scripts/verify.sh stream
```

The soak defaults to 90 minutes, 1k QPS, 64 concurrency, 1,000 mixed rules and
bounded trace. Release qualification requires at least 5 million requests, 90
resource samples, bounded RSS/FD growth, a last-half RSS slope no greater than
1MiB/hour, and zero trace loss or residue. Short smoke runs may lower
`RSPROXY_SOAK_MIN_ELAPSED_SECONDS`, `RSPROXY_SOAK_MIN_REQUESTS`, and
`RSPROXY_SOAK_MIN_SAMPLES`; they prove wiring and exact correctness, not the
steady-state gate. The Whistle driver installs its exact `whistle@2.10.5`
lock into `target/bench-deps/` on first use; `RSPROXY_WHISTLE_DIR` may point to
an existing matching installation. It defaults to enforcing every §9.3 target.

Run formatting and compile checks:

```sh
cargo fmt --all -- --check
cargo check --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo check --manifest-path fuzz/Cargo.toml --bin parse_resolve --locked
```

Windows-only branches remain best-effort compatibility code. Cross-target and
hosted target-OS validation are intentionally outside the current macOS-only
qualification workflow.

Run a finite sanitizer-backed rules fuzz smoke without writing generated inputs
into the checked-in corpus. This requires `cargo-fuzz` and a nightly toolchain:

```sh
RSPROXY_FUZZ_RUNS=1000 ./scripts/verify.sh fuzz
RSPROXY_FUZZ_RUNS=0 RSPROXY_FUZZ_SECONDS=60 ./scripts/verify.sh fuzz
RSPROXY_FUZZ_RUNS=0 RSPROXY_FUZZ_SECONDS=300 ./scripts/verify.sh fuzz
```

`RSPROXY_FUZZ_MAX_LEN` is bounded to 1-65536. The script rejects a zero run and
zero duration combination and always fuzzes a temporary copy of the checked-in
corpus. `.github/workflows/fuzz.yml` runs the 300-second form every day on
Ubuntu/nightly and uploads `fuzz/artifacts/parse_resolve` only after a failure.
The 2026-07-12 local workflow-equivalent run completed 463,561 executions with
no crash or artifact. After nested body deletion landed, a further 60-second
sanitizer run completed 121,726 executions with no crash or artifact.

The repository keeps Rust source files at 500 lines or fewer. Check that invariant with:

```sh
cargo xtask check lines
```

Test placement is also a repository invariant. This rejects inline test modules,
test functions outside dedicated test paths, and crates without a public
integration-test directory:

```sh
cargo xtask check layout
```

Typed errors are a repository invariant. This AST check scans both workspace and
fuzz sources and has no exemption list:

```sh
cargo xtask check typed-errors
```

Workflow files are also a repository contract. This checks their exact inventory,
YAML syntax without an external interpreter, least-privilege token policy, stable
action references, triggers, matrix platforms and required commands:

```sh
cargo xtask check workflows
```

`ci.yml` runs locked check/test/release builds on Ubuntu, macOS and Windows. Its
Ubuntu jobs additionally run formatting, Clippy, source/test/workflow guards,
coverage, the fuzz-target compile check, the dual-package 34-owner protocol
matrix and the action-effect suite. `performance.yml` owns Criterion
comparison. `release.yml`
owns the npm registry pipeline for eight native packages, `@rsproxy/runtime`,
`@rsproxy/cli`, and `@rsproxy/bun`. The fast package contract runs under both
Node and Bun and installs only the current-host fixture. The supply-chain job
runs cargo-deny across advisories, licenses, bans and sources. Clippy runs with
all default warnings denied and no project-wide lint exception.
