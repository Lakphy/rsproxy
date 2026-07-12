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

```text
crates/rsproxy-cli/
  src/cli/help.rs                    分层 usage 与副作用前 help dispatch
  src/cli/tests/                     CLI 参数、配置与命令适配器白盒测试
  src/control/tests/server.rs        控制客户端正常断连日志分类
  src/app/tests/                     运行期缓存容量、LRU 与 TTL 测试
  src/rule_store/tests.rs            分组迁移、启停、持久化与快照隔离
  src/rule_store/watch/tests.rs      文件事件、debounce、失败恢复与退出
  src/http/tests/                    HTTP wire-format unit tests
    buffered_head.rs                 buffered 响应头批读与 body 保留边界
    tcp_head.rs                      TcpStream peek 快读与 pipeline/body 保留
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
  src/h2/tests/                      h2 wire message/body conversion
  src/transfer_timing/tests.rs       单调传输计时器的 EOF/drop/冻结语义
  src/upstream_body/tests.rs          bounded frame collection and continuation
  src/proxy/tests/h1_forward.rs      pooled HTTP/1 framing, reuse and trace behavior
  src/upstream_h2/tests/             message, request body, streaming, pool and timeout behaviors
  tests/                             executable/public black-box tests
    cli_help.rs                       所有 command/subcommand 帮助快速退出且无副作用
    cli_completions.rs                Bash/Zsh/Fish/PowerShell 生成与错误合同
    cli_daemon_lifecycle.rs           真实 daemon 启停/重启/恢复/bind/PID 身份矩阵
    cli_json_contracts.rs             查询 JSON shape 与单文档错误 schema
    cli_product_matrix/               values/CA/proxy/trace/replay/TUI 产品路径
    cli_logging.rs                    stderr NDJSON 启动/监听事件与端口 0 黑盒合同
    large_stream_resource.rs         release 代理 1GiB、RSS 与 trace 资源验收（显式运行）
  examples/
    bench_origin.rs                  固定 1KiB keep-alive 本地 origin
    bench_client.rs                  并发持久连接、CL/chunked 响应 benchmark client
crates/rsproxy-rules/
  src/tests/                          private parser/resolver unit tests
    body_planning.rs                  candidate body-dependency planning
    conditions/                      request-period and response-period conditions
  tests/corpus/                       86 个 matcher/condition/action/composition/error case
  tests/corpus.rs                     runner 与 37 个 DSL-spec 双向锚点
  tests/properties.rs                 生成式合法/近似合法/任意输入不变量
  tests/fuzz_seeds.rs                 cargo test 中回放版本化 fuzz seeds
  tests/complexity.rs                 64KB hostile/scaling 时间预算门禁
  tests/value_sources.rs              公开 Value 语法与 key 边界
  tests/value_matrix.rs               40 个结构化值字段的四来源矩阵
  tests/contracts/                     非 corpus 的迁移/option TOML 合同
    whistle_migration.toml             whistle 96-name 源注册表与 46-family 映射
    whistle_options.toml               56 enable/66 disable/16 delete 分类
  tests/fixtures/whistle-2.10.5/       75 个只读上游证据、MIT 许可与 SHA-256
  tests/whistle_migration.rs           源注册表与 action 映射 runner
  tests/whistle_options.rs             文档 option 分类与实现配方 runner
  tests/support/fuzz_harness.rs        seed test 与 libFuzzer 共用 harness
  tests/                              public RuleSet API tests
crates/rsproxy-trace/
  src/tests/collector.rs              队列溢出、查询屏障、内存预算、并发与关闭
  src/tests/events.rs                 增量事件、乱序/并发、丢弃校正与 pending 预算
  src/tests/spill_read.rs             collector 外读取、append/clear/eviction 快照竞态
  src/tests/mod.rs                    spill 轮转、恢复、压缩与损坏记录
  tests/                              public TraceStore API tests
fuzz/
  fuzz_targets/parse_resolve.rs       parse/resolve nightly sanitizer target
  corpus/parse_resolve/               valid/invalid 可审阅 seeds
benches/e2e/benchmark.sh              release 代理 + curl + Rust client 宏基准
benches/e2e/performance.sh            oha 吞吐、延迟、RSS 版本化报告
benches/e2e/whistle.sh                同机 Whistle pureProxy 严格对比
benches/e2e/whistle-driver/           固定 2.10.5 的独立 npm lock
benches/soak/soak.sh                  参数化 90m/QPS/规则/trace 稳态驱动
benches/criterion/                    rules/trace/certificate 微基准与报告收集
packages/npm/tests/                   npm/Bun 平台映射、版本和 manifest 合同
scripts/verify.sh package             本机 npm/Bun pack/install/launcher 黑盒
scripts/verify.sh coverage-report                   llvm-cov 生产代码覆盖率门禁
scripts/verify.sh actions        action corpus、迁移和网络效果统一验收
scripts/verify.sh matrix       34 项精确协议 owner 与防漂移验收
scripts/verify.sh bench             benchmark JSON 合同验收
.github/workflows/ci.yml              Ubuntu/macOS/Windows workspace 与 Ubuntu 合同门禁
.github/workflows/fuzz.yml            Ubuntu nightly 每日 sanitizer fuzz
.github/workflows/performance.yml     同 runner base/current Criterion 回归
.github/workflows/release.yml         八个原生 npm 包与两种启动器 tag 发布
scripts/check.sh workflows            workflow inventory、语法和必跑命令静态合同
```

The larger suites are grouped by behavior rather than by implementation function:

- `rsproxy-cli/src/cli/tests/`: API auth, CA, runtime options, rule request
  construction, TOML precedence/error handling and system-proxy command plans.
- `rsproxy-cli/src/control/tests/`: control authentication, query decoding and
  resource-route contracts, including ordered rule-group lifecycle.
- `rsproxy-cli/src/rule_store/watch/tests.rs`: atomic disk reload, bounded event
  queue, debounce, invalid-edit rollback, recovery and worker shutdown.
- `rsproxy-cli/src/proxy/tests/`: connection/auth, routing, TLS policy, WebSocket,
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
- `rsproxy-cli/src/proxy/h2_bridge/tests/`: bounded request-channel adaptation,
  incremental response framing/trailers, incomplete bodies, and body-forbidden
  HEAD/204 behavior.
- `rsproxy-cli/src/proxy/tests/h2_downstream_streaming.rs`: one real TLS+h2
  client connection proves that an oversized upload reaches the origin before
  the client finishes sending and that response head/DATA arrive before the
  origin completes, while preserving both request and response trailers.
- `rsproxy-cli/src/proxy/tests/origin_h2_streaming.rs`: real h1 and h2 clients
  prove that oversized uploads reach a TLS/ALPN h2 origin before client
  completion, with body-rule degradation, trace prefixes, exact byte counts and
  request trailers preserved.
- `rsproxy-cli/src/proxy/server/tests/`: deterministic policy precedence and
  non-consuming TLS/HTTP/unknown/timeout protocol detection.
- `rsproxy-cli/src/proxy/tests/connect_modes.rs` additionally verifies that a
  passthrough tunnel remains pending while copy is open, completes exact duplex
  byte totals, handles refusal and MITM timeout without orphan events, and never
  starts a trace for `hide`.
- `rsproxy-cli/src/proxy/tunnel/tests.rs`: verifies direction-aware byte events
  aggregate without retaining opaque tunnel payloads.
- `rsproxy-cli/src/proxy/tests/h1_forward.rs`: pooled HTTP/1 connection reuse,
  framing errors, SSE, close-delimited bodies and trace fidelity.
- `rsproxy-cli/src/upstream_h2/tests/`: wire conversion, real pooled gRPC
  transport, bounded request-body error/deadline behavior, cold and pool-hit
  streaming uploads, connector/stream admission and timeout scopes.
- `rsproxy-cli/src/transfer_timing/tests.rs` and the h1/h2 proxy tests verify
  one-shot timer freezing, EOF/drop behavior, independent slow upload/response
  intervals, and known-versus-unknown timing boundaries on transfer failures.
- `rsproxy-rules/src/tests/`: actions grouped by behavior, body-dependency
  planning, conditions, indexing and regular expressions.
- `rsproxy-rules/tests/corpus.rs`: runs 86 public cases and requires all 37
  specification anchors to resolve bidirectionally. Edge cases cover malformed
  authority/exact URL input, path/query glob boundaries, condition parameter
  validation, and response-dependent negation before/after a response snapshot.
- `rsproxy-rules/tests/properties.rs`: 256-case generated valid-rule reparse,
  structured near-valid failures and bounded arbitrary-input API traversal.
- `rsproxy-rules/tests/fuzz_seeds.rs`: replays the exact seed corpus used by the
  `parse_resolve` libFuzzer target through their shared harness.
- `rsproxy-rules/tests/value_matrix.rs`: parses 40 structured value slots with
  inline, template/capture, `@key`, and `<file>` sources (160 combinations).
- `rsproxy-rules/tests/value_sources.rs`: verifies public AST classification,
  quoted-literal behavior, key length/character boundaries, and parse errors.
- `rsproxy-rules/tests/whistle_migration.rs`: requires 46 source-backed supported
  mappings to cover exactly `Action::FAMILIES`, parses Whistle's 74 canonical
  protocols and 22 explicit aliases from the pinned 2.10.5 evidence fixture,
  and requires every name to be supported or explicitly deferred/removed.
- `rsproxy-rules/tests/whistle_options.rs`: extracts 56 `enable`, 66 `disable`,
  and 16 `delete` option classes from the same immutable fixture, requires an
  exact classification, parses/resolves every recipe marked implemented, and
  checks every `process-config` reference against real CLI help. Milestone-scoped
  deferred labels are rejected; out-of-v1 behavior must remain explicit v2.
- `rsproxy-rules/tests/complexity.rs`: exercises valid, malformed, many-rule,
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
- `rsproxy-cli/tests/cli_rule_groups.rs`: executable-level offline group
  set/list/disable/enable/test/remove lifecycle.
- `rsproxy-cli/tests/cli_trace_follow.rs`: executable-level live NDJSON follow,
  heartbeat handling and `--count` termination against a fake control API.
- `rsproxy-cli/tests/cli_logging.rs`: starts the real executable with ephemeral
  proxy/control ports, parses stderr NDJSON, and verifies stable startup,
  trust-root and bound-address events. It also prevents process logs from
  contaminating stdout command contracts.
- `rsproxy-cli/tests/cli_help.rs`: runs root, lifecycle, API, rules, values,
  trace, TUI, replay, CA, system-proxy and completion help for every supported
  subcommand through the real executable with a watchdog; help must succeed
  before runtime side effects, while unknown commands retain nonzero errors.
- `rsproxy-cli/tests/cli_daemon_lifecycle.rs`: starts detached real processes and
  verifies status, duplicate start, restart with rule retention, normal stop,
  abnormal-kill recovery, malformed pidfiles, occupied listener cleanup,
  ephemeral-port rejection and refusal to kill an unrelated live PID. Its
  Windows-only case uses the authenticated named-pipe transport.
- `rsproxy-cli/tests/cli_json_contracts.rs`: verifies exact query object keys and
  scalar shapes for rules, values, CA, status, trace and system-proxy plans;
  unknown, missing, unavailable and broken-config failures each emit one
  `rsproxy.cli.error/v1` document on stderr.
- `rsproxy-cli/tests/cli_product_matrix/`: splits offline values/CA/proxy and
  online trace/replay/TUI product paths into responsibility-named files behind
  one integration-test entry point.
- `rsproxy-cli/tests/cli_completions.rs`: validates Bash, Zsh, Fish and
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
cargo test -p rsproxy-rules --test corpus
cargo test -p rsproxy-rules --test complexity
cargo test -p rsproxy-rules --test properties
cargo test -p rsproxy-rules --test fuzz_seeds
cargo test -p rsproxy-rules --test value_matrix
cargo test -p rsproxy-rules --test value_sources
cargo test -p rsproxy-rules --test whistle_migration
cargo test -p rsproxy-rules --test whistle_options
cargo test -p rsproxy --lib control::tests::
cargo test -p rsproxy --lib proxy::tests::request_streaming::
cargo test -p rsproxy --lib proxy::tests::connect_modes::
cargo test -p rsproxy --lib proxy::tunnel::tests::
cargo test -p rsproxy --lib proxy::h2_bridge::tests::
cargo test -p rsproxy --lib proxy::tests::h2_downstream_streaming::
cargo test -p rsproxy --lib proxy::tests::origin_h2_streaming::
cargo test -p rsproxy --lib upstream_h2::tests::streaming::
cargo test -p rsproxy --lib transfer_timing::tests::
cargo test -p rsproxy --lib proxy::server::probe::
cargo test -p rsproxy --lib proxy::tests::timeouts::
cargo test -p rsproxy --lib proxy::tests::value_actions::
cargo test -p rsproxy --lib proxy::tests::value_runtime_matrix::
cargo test -p rsproxy --lib proxy::tests::action_effects::
cargo test -p rsproxy --lib proxy::request_util::tests::
cargo test -p rsproxy-trace --all-targets
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

The script inventories 34 exact owners before running them, so a renamed or
deleted test fails instead of reporting a successful zero-test filter. It
covers h1 persistence/pipeline/Expect/auth, CONNECT MITM/passthrough/probing,
h2 bridge directions and bounded duplex flow, request/response trailers,
framing and body limits, gRPC, SSE, WebSocket frame behavior, TLS/mTLS policy,
and h1/h2 header-limit parsers. Dedicated real-network owners additionally
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
errors. It proves the M0 script is runnable; it is not the pending criterion/oha
M5 performance threshold.

Run the complete workspace suite:

```sh
cargo test --workspace --all-targets --no-fail-fast --locked
```

The 2026-07-12 baseline is 445 regular passing tests plus the explicit 1GiB
resource test ignored by default. The latest explicit run transferred
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
./scripts/targets.sh criterion target/performance/criterion.json
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
./scripts/check.sh lines
```

Test placement is also a repository invariant. This rejects inline test modules,
test functions outside dedicated test paths, and crates without a public
integration-test directory:

```sh
./scripts/check.sh layout
```

Workflow files are also a repository contract. This checks their exact inventory,
YAML syntax when Ruby is available, least-privilege token policy, released action
majors, triggers, matrix platforms and required commands:

```sh
./scripts/check.sh workflows
```

`ci.yml` runs locked check/test/release builds on Ubuntu, macOS and Windows. Its
Ubuntu jobs additionally run formatting, Clippy, source/test/workflow guards,
coverage, the fuzz-target compile check, the 34-owner protocol matrix and the
action-effect suite. `performance.yml` owns Criterion comparison. `release.yml`
owns the npm registry pipeline for eight native packages, `@rsproxy/runtime`,
`@rsproxy/cli`, and `@rsproxy/bun`. The fast package contract runs under both
Node and Bun and installs only the current-host fixture. Clippy runs with all
default warnings denied and no project-wide lint exception.
