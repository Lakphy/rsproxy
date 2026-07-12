# Completion Audit

审计日期：2026-07-12

## 结论

当前 workspace 可以在目标环境 Apple M1 Pro / macOS ARM64 上稳定构建，445 项常规
测试全部通过，并且已经完成 97 轮 Dogfooding 记录。M0 的 workspace、代理直通、
前台运行、结构化日志、curl 和可运行基准脚本现已全部具备实现与运行证据；M1 的设计内
v1 DSL/action、corpus 与 10k 规则指标，M2 的协议实现、真实运行与 34-owner 自动化
矩阵，M3 的 Trace 实现和资源验收，以及 M4 的 daemon/CLI 与产品矩阵均已闭环。
M5 的本机性能、覆盖率、稳态资源和 macOS release 证据也已关闭。按当前本机验收
范围逐项审计，M0-M5 已完成；完成度为 100%。

当前 v1 不再要求 Linux/Windows 目标 OS 运行、hosted runner 或多平台发布产物。
已有跨平台实现、交叉编译结果和 workflow 继续保留为 best-effort 兼容能力，但它们
既不作为完成证据，也不再构成完成度缺口。

本审计只读取当前源码、manifest、测试和 Dogfooding 记录，不把设计意图、
历史描述或测试未报错当作完成证据。Loop 96 使用隔离 origin、release daemon、
真实 CLI 和 curl 闭环普通 HTTP、CONNECT/tunnel 与 Trace；Loop 97 又以强制 h1/h2
curl、TLS/h2-only origin 和 8MiB echo 闭环双协议流式、trailers 与 TLS origin
identity。随后未启动 Loop 98，而是在专用测试目录以真实 listener/client/proxy
关闭 WS、mTLS、header 和名称边界，并将其纳入 34-owner CI 矩阵。M1 action 验收
仍使用隔离的真实 TCP/TLS origin、代理和客户端，不把直接调用 transform 函数
算作网络效果证据。代码拆分后所有 Rust 文件均受 500 行上限约束，测试统一进入专用
测试路径；本轮只补充本机回归和文档，没有启动新的 Dogfooding 轮次。

## 当前基线

- `cargo fmt --all -- --check`：通过。
- `cargo check --workspace --all-targets --locked`：通过且无 warning。
- `cargo clippy --workspace --all-targets --locked -- -D warnings -A
  clippy::too-many-arguments`：通过；唯一项目级豁免是既有低层协议编排签名，其余
  默认 Clippy warning 均阻塞。
- `cargo test --workspace --all-targets --no-fail-fast --locked`：445 项通过、1 项显式
  1GiB 资源验收默认 ignored；CLI lib 295、rules lib 83、trace lib 28，其余为公开/
  可执行集成测试和公开 crate 合同。
- `scripts/test-large-stream-resource.sh`：显式 ignored 测试已用 release 代理完成
  1GiB 真实 TCP 传输；端到端和 trace 字节均为 1,073,741,824，4KiB preview 正确，
  queue 无丢弃、partial 无残留；最新用时 679ms，RSS 从 15,328KiB 到 18,336KiB，
  增长 3,008KiB。验收同时接受合法 Content-Length/chunked framing，但始终使用
  固定 64KiB 测试缓冲，不会在客户端聚合 1GiB。
- `scripts/test-benchmark.sh`：通过；release 代理先经 curl 验证 1KiB body，再由
  仓库内 h1 keep-alive client 完成 128/128 请求、精确 131,072 bytes、零 status/IO
  error。默认 `benches/e2e/benchmark.sh` 另完成 1000/1000 请求、1,024,000 bytes、
  零错误并输出 `rsproxy-benchmark/v1` JSON；该结果只证明 M0 脚本可运行。
- `cargo build --release --workspace --locked`：通过；当前验收产物为可运行的 macOS
  arm64 Mach-O，`rsproxy --version` 为 0.1.0。本机曾产出 Windows x64 GNU PE 和
  Linux x64 musl 静态 ELF 归档，但这些历史交叉编译结果不属于当前发布资格。
- `scripts/check-rust-lines.sh`：通过，最大 Rust 文件 484 行。
- `scripts/check-test-layout.sh`：通过，测试均位于专用测试路径。
- `scripts/check-workflows.sh`：通过；CI/performance/fuzz/release YAML、最小权限、
  action 版本、平台矩阵和必跑命令满足静态合同；四个 workflow 另通过 actionlint。
- `scripts/coverage.sh`：通过；排除 test/bench/example 后 workspace 19,690/23,145
  行（85.072%），rules 3,220/3,345 行（96.263%）。
- Criterion 收集 11 个指标；缓存 TLS 握手置信上界 299,744ns（目标 <3ms），相对
  上一份本机报告无指标回退超过 10%。
- 本机正式 oha 基线为 45,392 rps，direct 为 101,707 rps；按 10% 回归预算，本机
  发布下限为 40,853 rps。附加 p50 169.25µs、p99 869.29µs、空载 17,952KiB，
  Whistle `pureProxy` 同机倍率 76.4x，均通过本机 §9.3 验收线。
- 最新 10 秒稳定性 smoke 在 500 QPS、32 并发、1,001 条规则与 trace 开启时完成
  5,001/5,001 请求；RSS 峰值/结束增长 7,360/6,288KiB，FD 峰值/结束增长 35/3，
  pending/incomplete/orphan/queue drop/spill error 均为 0。
- `scripts/test-protocol-matrix.sh`：34 个精确 owner 全部通过；脚本先检查测试清单，
  缺失或改名会失败，不会把 exact filter 的零测试当成功。CI 与 workflow 合同均已
  纳入该入口。
- `cargo check --manifest-path fuzz/Cargo.toml --bin parse_resolve --locked`：通过；
  run-count smoke 和与定时任务相同的 300 秒 ASan/libFuzzer 均通过。后者执行
  463,561 次、零 crash；nested body delete 落地后的额外 60 秒回归又执行
  121,726 次、零 crash，均未改写 8 个版本化 seed 或留下 crash artifact。

以上是当前 macOS-only M0-M5 的可靠回归与验收基线。

## 里程碑状态

| 里程碑 | 状态 | 已有证据 | 未完成或证据缺失 |
| --- | --- | --- | --- |
| M0 骨架 | 完成 | workspace、h1、CONNECT、前台 `run` 和真实 curl 已有历史 Dogfood；`tracing`/`tracing-subscriber` 提供 stderr text/JSON 稳定事件，真实二进制测试验证监听地址；本地 release e2e benchmark 和 JSON 合同测试均已运行通过 | 无 M0 阻塞项；§9.3 的正式性能阈值属于 M5，不混入本项 |
| M1 规则引擎 | 完成 | §6.1/§6.3 的 matcher/when、46-family v1 action、完整 DSL 规范、86-case/37-anchor corpus A/C/D、40 字段七类值矩阵、有序分组/ArcSwap/watch、typed JSON/form/JSONP delete、proptest/fuzz/64KB 复杂度门禁和稳定错误码均已落地；46-family 真实网络效果严格一 owner；10k 规则 release Dogfood p99 3.458µs，低于 10µs 验收线；Whistle 96-name 注册表与 56/66/16 option 合同无遗漏 | 无 M1 阻塞项；设计明确采用全新 DSL 而非 Whistle 语法兼容，超出 §6.3 v1 action 集的插件、压缩、frame 控制等继续显式 `deferred-v2`，不伪装成近似 v1 action |
| M2 MITM + 全协议 | 完成 | CA、TLS MITM、h1/h2、WS、SSE、gRPC、trailers、mTLS、代理认证、上游代理链及主要 v1 actions 已实现并 Dogfood；no/strict MITM、pinning TTL 重试降级及 TLS/明文 HTTP/未知协议探测有自动化 socket 证据；Loop 96 验证 no-MITM CONNECT/TLS，Loop 97 以强制 h1/h2 curl、8MiB echo 和 origin trailers 关闭真实 h1→h2/h2→h2 流式证据；34-owner 矩阵进一步以真实网络覆盖 WS server-first/双向帧、mTLS 成功/匿名失败、h1/h2 200KB/超限 431、IPv6 literal 与 punycode route | 无 M2 阻塞项；v2+ 的 CONNECT/WebSocket over h2、h2c、SOCKS5 入站和 HTTP/3 不混入 v1 验收 |
| M3 Trace | 完成 | HTTP 与 CONNECT/tunnel 生命周期事件、`Bytes` 非阻塞 collector、pending 聚合、最终快照校正、queue/resident 总字节预算、环驱逐、独立 request-send/response-receive、collector 外 spill 快照/CRC/zstd 导出、索引和磁盘预算均已实现；1GiB/RSS release 验收通过，Loop 96 又以真实 CLI/curl 闭环 live follow、HTTP/tunnel session、timing、stats、spill、JSON/HAR export、TUI 和 replay | 无 M3 阻塞项；后续新增 Trace 功能仍需按同一证据标准验收 |
| M4 CLI 完备 | 完成 | daemon 同步预绑定并监督 proxy/control listener，启动/重启/停止、规则保留、异常 kill、损坏 pidfile、端口冲突与 PID 身份防误杀均有真实二进制矩阵；查询 JSON 与单文档错误合同、全部层级 help、Bash/Zsh/Fish/PowerShell completions、values/trace/replay/TUI/CA/proxy 产品矩阵均已落地；本机 macOS `networksetup` / `security` 为正式路径；Linux/Windows 分支继续保留 | 无 M4 阻塞项；Linux/Windows 原生命令和 named pipe 的目标 OS 运行已移出当前 v1 验收范围 |
| M5 打磨发布 | 完成 | Criterion/oha/Whistle/soak/coverage 驱动和 >10% 回归门禁均已固化；本机正式基线 45,392 rps，回归下限 40,853 rps，最佳 54.3k rps；覆盖率 85.072%/96.263%，TLS、规则、延迟、RSS、Whistle 76.4x 和 1GiB 指标通过；高效稳态 soak 持续 6,307 秒、覆盖 6,379,936 个 session 与 106 个分钟样本，RSS 后半段斜率为负且 Trace 无丢失；macOS arm64 release 可运行并可打包 | 无本机 M5 阻塞项；hosted CI、Linux/Windows 运行和多平台产物不属于当前验收范围 |

## 确定性源码证据

### 规则与热更新

- `crates/rsproxy-cli/src/rule_store.rs` 通过 ArcSwap 发布包含完整分组和编译
  索引的快照；`rule_store/storage.rs` 管理 `groups.toml`、旧目录发现和原子
  文件替换。请求 body 计划、请求期和响应期共享同一快照。
- CLI/API 已覆盖命名分组 set/cat/edit/list/remove/enable/disable，且在线、离线
  `rules test/stats/bench` 都使用完整启用集合。`rule_store/watch.rs` 使用
  `notify` 监听外部 `*.rules`/`groups.toml` 变更，经可配置 trailing-edge
  debounce 后整组编译并原子发布；非法磁盘状态保留旧快照。
- watcher 回调只向容量 64 的通道执行 `try_send`，事件噪声在入队前过滤，队满
  时保留至少一个整目录重载触发并累计 dropped counter。`/api/status` 暴露事件、
  丢弃、成功重载、失败和最后错误；API 自身写入产生的事件以分组相等比较去重。
- `crates/rsproxy-rules/src/template/` 已拆分稳定元数据、渲染和变换解析，覆盖
  v1 的 20 个变量、`${var.replace(...)}`、regex `$0-$9`/命名捕获，以及请求/
  响应 header 和 cookie。响应动作共享一个只读 `Arc<ResponseMeta>`。
- `rules test --response-status/--response-header` 已同时接入控制 API 和离线
  fallback，可演练响应期条件、`${statusCode}`、`${resH.*}` 和
  `${resCookies.*}`；`rules bench` 保持请求期语义。
- `crates/rsproxy-rules/src/action/value.rs` 统一定义
  `Value::{Inline,File,Reference}` 和 1-128 字符 key 合同；parser 不再让
  `@key`/`<path>` 以普通字符串流入代理。`proxy/transforms/values.rs` 集中处理
  storage/file 读取、UTF-8 文本错误、模板/捕获渲染和二进制保真。key 在解析期
  和运行期双重验证；受信规则的 `<path>` 保留显式文件系统能力。
- `HeaderOp` 已支持 set/remove/regex replace；正则在解析期验证并缓存，代理层
  对所有同名 header value 按规则顺序应用，流式响应不因此聚合 body。
- `host` 已使用每规则地址池、共享原子游标和每次 resolve 的惰性选择缓存；并发
  请求轮询均衡，同一请求的 trace planning 与实际转发复用同一个目标。
- `RuleError` 已公开稳定 `syntax/matcher/action/condition/property` code 及
  group/line/message；控制 API 返回结构化错误对象。
- `Action::FAMILIES` 公开 46 个稳定 family；`actions.toml` 声明必须覆盖的全集，
  corpus runner 同时比对实现、声明和实际 resolve 结果，防止新增动作漏测。
- `rsproxy-rules/tests/corpus/` 已有 86 个 action/matcher/condition/composition/error/template
  case，并与 DSL 规范中的 37 个 matcher/condition/action/composition 锚点双向校验。
  matcher authority、exact URL、scheme/port 和条件参数在发布前严格验证；negated
  `status`/`res.header` 及其嵌套表达式在没有响应快照时保持 deferred。
  `properties.rs` 每项执行
  256 个生成样本，覆盖合法规则重解析、近似合法错误结构和任意有界 UTF-8 的
  parse/resolve/explain 无 panic 不变量。
- `value_matrix.rs` 对 40 个结构化值字段执行 inline、模板/捕获、`@key`、
  `<file>` 共 160 个解析组合；`value_sources.rs` 覆盖公开 AST、引号字面量和
  key/file 错误边界。代理 `value_actions.rs` 覆盖请求/响应/URL/路由/mock/body/
  trace 的引用与文件行为、编号/命名捕获、正则替换、UTF-8 和二进制边界。
- `value_runtime_matrix/` 对 40 字段逐一执行 basic、quoted、`@key`、`<file>`、
  模板、编号/命名捕获和非法 key，共 280 个运行时解析组合。它验证集中 resolver；
  实际 action 效果仍由 `value_actions.rs` 的跨类别用例验证，不把两者混为一谈。
- `proxy/tests/action_effects/` 通过真实 `handle_client`、TCP origin/client、TLS
  ClientHello 和完整 `RuleSet` 验证 46 个 family 的可观测网络效果。owner 集合与
  `Action::FAMILIES` 做严格相等及去重检查；`scripts/test-action-effects.sh` 将
  action corpus、迁移矩阵和 17 项网络验收收束为一个入口。该套件暴露并修复了
  流式响应按 frame 重置限速的问题；共享 `ThrottlePacer` 现在跨写入持续计速、
  遵守绝对请求 deadline，并防御程序化零速率。
- `tests/contracts/whistle_migration.toml` 的独立 runner 除 46 个支持 family 外，还直接解析
  `tests/fixtures/whistle-2.10.5/lib/rules/protocols.js` 的 74 个 canonical protocol
  和 22 个显式 alias；
  每个源注册名必须映射到支持的 action/语法，或被明确标记 deferred/removed。
  这证明注册表无遗漏，不代表所有复杂 option 已具备行为等价。
- `tests/contracts/whistle_options.toml` 与独立 runner 直接抽取英文 Whistle 文档，
  对 56 个 `enable`、66 个 `disable` 和 16 类 `delete` option 做严格无重无漏分类；
  `implemented` 配方必须由 rsproxy parser/response resolver 执行；`process-config`
  项必须引用真实存在于 CLI help 的进程开关，runner 不再接受任何
  `deferred-m1/m2/m4` 里程碑标签。超出 §6.3 的行为只允许明确归入 v2/removed，
  防止用近似 action 或过期阶段标签掩盖范围。新增 typed
  `DeleteOp` 覆盖 pathname/segment、全部或指定 URL 参数、双向 header/cookie/body、
  Content-Type type/charset 和 trailer；typed body path 进一步覆盖请求 JSON/form、
  响应 JSON/JSONP、转义 key 和数组索引。真实网络测试同时观察 origin 与 client，
  bounded planner 在超限时只跳过 body 相关删除而保留其余效果。
- `fuzz/fuzz_targets/parse_resolve.rs` 与 `fuzz_seeds.rs` 复用同一 harness 和 8 个
  可审阅 seed；nightly ASan/libFuzzer 已完成 1000 次 smoke、300 秒/463,561 次
  本地定时任务等价运行及本次变更后的 60 秒/121,726 次回归，均无 crash。
  `scripts/fuzz-rules-smoke.sh` 使用临时 corpus，支持 run-count/持续秒数并限制
  最大输入不超过 64KB。`complexity.rs` 对最大 inline、多规则、恶意 delimiter、
  fancy backtrack 和 8x scaling 设置自动化预算。`.github/workflows/fuzz.yml` 每日
  在 Ubuntu/nightly 运行 300 秒并在失败时保留 crash artifact。Criterion 已由独立
  performance workflow 负责，避免把 nightly sanitizer 与稳定计时混在同一 job。

### MITM 与流式边界

- `proxy/server/connect_policy.rs` 明确实现 no-MITM、规则 bypass、无 CA、失败
  cache 和 inspect 的优先级；`app/mitm_failures.rs` 提供 host 归一化、TTL、LRU
  和容量上限。CLI/TOML/status 已暴露 no/strict、容量、TTL 和探测时限。
- `proxy/server/probe.rs` 使用 socket `peek` 非消费地区分 TLS、明文 HTTP 和未知
  协议；`inner_http.rs` 让明文与 MITM HTTP/1 共享请求循环。未知协议/探测超时
  透传；TLS 非超时失败在 auto 模式记录 host，下一次 CONNECT 重试透传，strict
  模式不记录。`proxy/tests/connect_modes.rs` 已覆盖真实本地 socket 生命周期。
- `proxy/connect.rs` 将 `UpstreamRoute` 仅用于 TCP/proxy-hop 拨号，并始终把
  `UrlParts.host` 交给 origin TLS 作为 SNI/证书身份。Loop 97 首次请求暴露
  `host(...)` 曾把拨号地址误当 TLS 主机；修复后，命名 origin 可路由到字面 IP 而
  不改变 authority 或证书校验名，h1→h2、h2→h2 和双向 h2 fixture 均固化该语义。
- `proxy/h2_bridge/request.rs` 把 Hyper DATA/trailers 通过容量固定的 channel
  暴露给既有有界 request-body planner；超过 `body_buffer_limit` 后从同一读取
  位置流式续传，跳过 body-dependent 规则并保留 trace 前缀和 trailers。
- `proxy/h2_bridge/response.rs` 在 blocking 管道产出响应头后立即发布 h2 head，
  再增量解码 Content-Length、chunked/trailers 或 close-delimited body 到容量固定
  的 channel；HEAD/1xx/204/304 关闭下游 body 并丢弃内部管道字节。
- `proxy/tests/h2_downstream_streaming.rs` 使用真实 CONNECT、TLS ALPN 和同一 h2
  客户端连接，确定性证明请求在客户端发送完成前到达 origin、响应在 origin
  发送完成前到达客户端，并验证双向 trailers、规则降级和 trace 截断。独立的
  1GiB release 验收当前覆盖普通 h1 大响应；h2 资源证据仍是该有界提前到达测试。
- `upstream_h2/request_body.rs` 与 `streaming.rs` 提供容量 8、受 request-total
  deadline 约束的 DATA/trailer/error 通道；cold connector 和 pool-hit 都在读取
  完整客户端 body 前启动请求。h2 stream lease 由请求发送和响应 body 共同持有，
  两侧都结束后才释放本地并发配额。
- `proxy/tests/origin_h2_streaming.rs` 让 h1 与 h2 客户端分别经 CONNECT/MITM
  上传 1.125MB 到只宣告 h2 的 TLS origin；origin 在客户端发送剩余 1MB 前已收到
  DATA，最终字节、trailers、规则降级与 trace 均一致。`upstream_h2/tests/streaming.rs`
  另覆盖同一 h2 session 的 pool-hit 流式请求。
- Loop 97 以 curl 强制 h2 和 h1，分别经 MITM 向 TLS/h2-only `nghttpd` 上传并回显
  8MiB；两次请求均为 200、双向字节和 SHA-256 精确一致、origin trailer 保真，
  trace preview 固定 4KiB 且无 drop/partial/orphan。h2 路径采样 RSS 仅增长 7,280KiB。
  `scripts/test-protocol-matrix.sh` 又把 34 个精确 owner 收束到一个 CI 入口，并先
  校验 test list，防止测试改名后 exact filter 零执行仍返回成功。
- `proxy/tests/protocol_matrix/` 按 `websocket`、`mtls`、`headers`、`names` 四个职责
  文件组织 5 个真实网络用例。WS 用真实 upgrade、server-first 和双向 masked/unmasked
  frame 验证 trace；mTLS origin 用 client verifier 同时证明配置证书成功、匿名连接
  被拒并返回 502；h1/h2 各验证 200KB 通过及超限 431；IPv6 `::1` 与 punycode host
  route 验证 URL、Host、拨号地址和 trace identity。首次运行因此修复了 IPv6 URL
  重建丢方括号，以及 h2 transport limit 抢先 RST、应用层无法返回 431 的问题。

### Trace 热路径

- `crates/rsproxy-trace/src/event.rs` 公开 Start/Request/Response/BodyChunk/
  BodySnapshot/Frame/Tls/End/Abort；`store.rs` 只做原子 ID、预算预留和有界
  `try_send`。队满、queue 字节超限或断开立即累计计数，代理线程不触碰 spill I/O。
- `store/pending.rs` 在单消费者内聚合 partial session、限制双向 body preview、
  统计 incomplete/orphan 并清理 5 分钟无进展会话。收尾 continuation batch 的
  `BodySnapshot` 覆盖增量计数，可校正此前被队列丢弃的 chunk；`End` 同时校正
  WebSocket/SSE 等运行期确定的最终 kind。
- HTTP 热路径在规则/hide 决策后发送 Start/Request；流式 h1/h2 请求、池化 h1/h2
  响应和手写 h1 SSE 发送 body 事件。h2 复用 `Bytes` slice，h1/SSE 只复制尚未
  达上限的 preview。passthrough CONNECT 在确定进入 tunnel 后启动同一生命周期，
  双向 copy 只发送空 preview 与 observed byte count；失败/超时/空连接也经
  Start/Request/continuation 收尾。代理生产路径不再调用兼容 `record(Session)`。
- 256MB 总预算固定划分 queue/resident（默认 64MB/192MB）。queue 按事件动态
  数据与 observed chunk 字节原子计量，resident 同时覆盖 pending 和 completed；
  partial 超限中止，completed 超限驱逐最旧 session。stats 暴露所有分区、丢弃和
  partial/follow 指标。
- `store/worker.rs` 是 pending、内存环、spill 和 follower 的唯一 owner；查询命令
  共享 FIFO 屏障。follow 在同一 FIFO 中取得 backlog 并注册独立有界 subscriber，
  慢消费者只丢自己的记录。`TraceFollow` 的强 liveness token 与 worker 弱引用使
  客户端退出后的下一次 stats 立即清理订阅者，不再等待下一条 session。控制路由
  和 CLI 使用 live NDJSON、heartbeat 与 count 终止，已有真实 TCP 路由测试、
  可执行黑盒测试和 Loop 96 运行证据；正常 BrokenPipe/reset 被记为 debug，而真正
  控制请求故障仍为 WARN。
- `tests/large_stream_resource.rs` 与 `scripts/test-large-stream-resource.sh` 启动 release
  代理和真实 origin/client，以固定 64KiB 缓冲解码 1GiB Content-Length/chunked 响应
  并采样进程 RSS；默认 96MiB 增长门槛下最新增长 3,008KiB，同时验证
  trace/preview/drop/partial/总预算。
- `transfer_timing.rs` 用共享单调 one-shot timer 包装 Hyper request body；h1/h2
  response pump 通过 timed `UpstreamBody` 在 EOF/error 冻结 receive。手写 h1 在
  request write 与 response read 边界显式计时。nullable timing 已贯通 Session、
  `TraceEvent::End`、pending、JSON/spill、TUI 和 HAR；HAR 标准 send/wait/receive
  在 h2 双工重叠时投影到顺序预算，扩展保留 exact receive、overlap 和 residual。
  慢 h1/h2 上传、慢响应、EOF/drop、响应头超时、响应体错误与 HAR 双工闭合均有
  确定性测试。
- tunnel 迁移测试覆盖 copy 期间 pending、双向精确字节、无 opaque payload
  preview、连接拒绝、隐藏规则、MITM 握手超时及 orphan/partial 归零。
- spill 导出命令只在 collector 内初始化并打开段/索引句柄，复制不可变长度边界；
  CRC、zstd 解压和结果拼接在查询调用线程执行。并发测试证明读取暂停期间仍可
  record/stats，后续 append 不混入快照，clear/预算驱逐不破坏已捕获窗口；generation
  阻止 clear 前的过期 corruption 结果回写。
- Loop 96 在 `127.0.0.1:18961/18962` 启动 release daemon，以真实 curl 通过代理
  请求 Rust HTTP origin 和受本地 CA 信任的 TLS origin。`trace follow --count 2`
  依次收到 1,024-byte HTTP session 与双向 489/6,254-byte no-MITM tunnel session；
  stats 无 drop/partial/orphan/spill error，zstd spill snapshot 与 JSON export 含两条，
  HAR 按合同只含 HTTP。TUI 展示分段 timing，replay 返回 1,024 bytes。修复后的单条
  follow 在没有后续 publish 时即显示 `follow_subscribers=0`，且默认 info 日志无
  预期断连 WARN。

### 进程可观测与 M0 基准

- `logging.rs` 是唯一进程日志初始化边界；`RSPROXY_LOG` 优先于 `RUST_LOG`，
  `RSPROXY_LOG_FORMAT` 支持 text/JSON，所有日志固定走 stderr。监听、daemon、trust
  roots、连接错误及 session 成败均有稳定 `event` 字段，请求 Trace 仍由独立
  collector 管理。
- `tests/cli_logging.rs` 启动真实二进制和两个端口 0 监听器，从 stderr 解析 NDJSON，
  验证 `daemon_started`、proxy/control bound、trust roots 和实际非零端口。该测试
  曾发现 subscriber 默认 writer 会污染 stdout，修复后成为回归合同。
- `cli/help.rs` 集中根命令与子命令 usage；dispatch 在配置加载、token 发现和任何
  daemon/API/platform 操作前拦截帮助。`tests/cli_help.rs` 逐项覆盖所有顶层与嵌套
  command，并用 watchdog 证明帮助快速退出且不创建 storage；未知命令仍返回非零
  错误。`cli/completions.rs` 生成 Bash、Zsh、Fish 和 PowerShell 脚本，真实二进制
  测试验证输出且同样无运行时副作用。
- `tests/cli_daemon_lifecycle.rs` 使用隔离 storage 和真实子进程覆盖 start→status→
  restart→stop、规则保留、重复启动、异常 kill 后恢复、损坏 pidfile、自选端口
  拒绝、listener bind 失败清理，以及 pidfile 指向无关活进程时拒绝误杀。proxy 与
  control listener 在子进程进入常驻前同步绑定；任一 listener 退出会结束 daemon，
  readiness 同时校验已鉴权 status、storage 与 proxy identity。
- Unix 默认 control endpoint 随 storage 使用 `run/ctl.sock` 并强制 0600；长临时目录
  首次暴露 `sun_path` 上限后，默认解析增加 UID+storage SHA-256 短路径 fallback，
  lifecycle 测试验证实际 socket、peer auth 与 stop 清理。
- `tests/cli_json_contracts.rs` 对离线 rules/values/CA、在线 status/trace、三平台
  proxy dry-run 和 unknown/missing/unavailable/broken-config 错误执行真实二进制。
  查询输出保持单一 JSON 文档；带 `--json` 的失败只在 stderr 输出
  `rsproxy.cli.error/v1`。`tests/cli_product_matrix/` 进一步覆盖 values CRUD、CA
  生命周期、trace clear/JSON+HAR export、replay、TUI text/JSON snapshot 和三平台
  mutation plan。
- `benches/e2e/benchmark.sh` 使用仓库内固定 1KiB origin 和 CL/chunked 兼容的 Rust
  h1 client，自动提取结构化监听地址并先执行 curl。`scripts/test-benchmark.sh`
  校验版本、完成数、精确字节和零错误。它关闭 M0 的“基准脚本可跑”，不声称
  达到 §9.3 或具备可跨机器比较的性能基线。

### M5 性能、覆盖率与长稳

- `benches/e2e/performance.sh` 用同一 1KiB origin 分别测 direct 与 release proxy，
  输出吞吐、p50/p99 差值、空载/满载 RSS 和可选 Whistle 倍率。h1 热路径增加
  TcpStream peek 请求头批读、BufReader 响应头批读、无 key 重分配的小容量线程池，
  模板元数据改为共享惰性初始化；50k/16 从本轮早期约 8.4k 提升到 45-50k rps，
  最佳 50k/32 为 54.3k rps。本机正式发布基线采用 50k/16 的 45,392 rps，允许
  10% 回归后的最低值为 40,853 rps；原 80k/通用 8c 目标已移出当前验收范围。
- `benches/e2e/whistle.sh` 固定 Whistle 2.10.5 `pureProxy`，先以 10k 请求避免其
  不复用 origin 导致本机临时端口耗尽，并严格要求 100% 成功/精确字节。最新报告
  Whistle 594.0 rps、rsproxy 45,392.1 rps、76.4x；rsproxy 附加 p50 169.25µs、
  p99 869.29µs、空载 17,952KiB。以 `RSPROXY_PERF_MIN_RPS=40853` 执行本机 checker
  时全部指标通过。完整上游源码已移除；脚本通过独立 npm lock 按需安装到
  `target/bench-deps/`，合同测试仅保留 75 个带哈希的上游证据文件。
- `benches/criterion/run.sh` 收集 rules parse/resolve、trace enqueue、证书签发/缓存和
  缓存 TLS 握手共 11 项。`check-performance-regression.sh` 拒绝缺失指标和超过 10%
  的 mean 回退；`performance.yml` 在同一 hosted runner 顺序测 base/current，降低
  跨机器噪声。当前 10k mixed resolve 约 0.99µs，TLS 上界约 0.300ms。
- `scripts/coverage.sh` 与 CI coverage job 执行 llvm-cov，按生产代码行计算并严格
  门禁 workspace ≥85%、rules ≥95%；最新为 85.072%/96.263%。CONNECT/SOCKS5、
  非阻塞 WS、系统代理解析、TCP/buffered header 和 `--version` 等缺口均有专用测试。
- `benches/soak/soak.sh` 默认采用高效稳态口径：90 分钟、1k QPS、64 并发、1,000
  条混合规则和 trace；除精确请求、RSS/FD 和 Trace 门禁外，还要求至少 500 万请求、
  90 个样本，并限制后半段 RSS 斜率。实际运行 6,307 秒，覆盖 6,379,936 个 session
  和 106 个分钟样本；RSS 从 31,440KiB 到 28,624KiB，峰值 37,600KiB（+6,160KiB），
  后半段斜率 -3,928.745KiB/h，FD 峰值 136/上限 144，pending/incomplete/orphan/
  queue drop/spill error 均为 0。进程和临时目录最终全部清理；汇总报告与原始样本
  保存在 `docs/evidence/macos-efficient-soak*.{json,tsv}`。

### 本机发布与非阻塞兼容代码

当前发布资格只覆盖 Apple M1 Pro / macOS ARM64。以下 Linux/Windows 内容记录已有
实现与历史交叉编译事实，不要求继续验证，也不参与完成度计算。

- `cli/system_proxy/` 按平台拆分：macOS 使用 `networksetup`；Linux 读取并写入
  GNOME `gsettings`，写入前保存原值且中途失败逆序回滚；Windows 写入当前用户
  Internet Settings registry，失败恢复 `ProxyEnable/ProxyServer/ProxyOverride`，
  成功后通过 WinINet 通知刷新。三者均保留无副作用 `--dry-run` 和 JSON plan。
- `cli/ca/trust/` 将 Linux p11-kit `trust anchor` 与 Windows 当前用户 Root store
  `certutil` 从 macOS `security` 中拆开；Windows 卸载使用证书 SHA-1 thumbprint，
  所有结果仍回显 SHA-256 identity，三平台都支持 dry-run 审计。
- `windows_pipe.rs` 独占 Win32 handle，提供同步 `Read + Write` server/client；首实例
  使用 `FILE_FLAG_FIRST_PIPE_INSTANCE`，只接受本机连接，并继续要求 storage token
  鉴权。`control.rs` 与 CLI API client 共用该 transport；Windows 默认 API 为
  `pipe:rsproxy-control`，TCP 仍可显式选择。Windows daemon test 以 `cfg(windows)`
  覆盖 start/status/stop 与 token mode。
- 本机安装 MinGW 后，`x86_64-pc-windows-gnu` 的全 target check、Clippy
  `-D warnings` 和 release link 均通过；这证明目标代码可编译链接，不冒充 hosted
  Windows 上执行了 platform mutation 或 named-pipe test。
- `scripts/package-release.sh` 校验目标/版本，打包二进制、README、MIT License 并
  生成 SHA-256；`test-release-package.sh` 覆盖内容、checksum 和版本不匹配失败。
  `rsproxy --version`/`-V` 已由真实二进制黑盒测试保证，可用于产物 smoke。
- 本机最新源码已实际产出并检查：可运行的 `aarch64-apple-darwin` 归档、PE32+
  `x86_64-pc-windows-gnu` 归档，以及无 ELF interpreter 的 statically linked
  `x86_64-unknown-linux-musl` 归档；每个 checksum 均通过。
- `.github/workflows/ci.yml` 定义三平台 locked check/test/release 和 Ubuntu coverage/
  contracts；`performance.yml` 定义同 runner Criterion；`fuzz.yml` 定义每日
  sanitizer；`release.yml` 定义 Linux GNU/musl、macOS arm64/x64、Windows MSVC
  五目标打包与 tag assets。静态合同、YAML 和 actionlint 已通过；是否接入远端或
  执行 hosted workflow 不影响当前本机发布资格。

## 文档状态修正

技术设计现已把“系统代理 / CA 安装”的正式验收路径限定为本机 macOS；Linux/Windows
平台分支、编译合同与产品矩阵是保留兼容能力，不再追踪 hosted 目标 OS 运行证据。

此前仅有 corpus 的 IPv6/punycode 项已由真实 `::1` origin 和 punycode
`host(...)` 路由测试关闭；M2 不再依赖该文档标记自证。

已明确放到 v2+ 的 SOCKS5 入站、CONNECT over h2、RFC 8441、h2c、插件和
HTTP/3 不属于本次 v1 完成阻塞项；不得为了追求“全部”把这些范围重新塞回
M0-M5。

## 当前剩余项

当前 macOS-only M0-M5 无剩余阻塞项。不再安排 Linux/Windows/hosted 多平台验证，
也不为此启动新的 Dogfooding 轮次；后续工作进入独立的新需求或 v2 范围。
