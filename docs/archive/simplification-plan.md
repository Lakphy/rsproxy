# rsproxy 代码简化实施记录

> 状态：已实施（2026-07-12）  
> 资格范围：当前 Apple M1 Pro / macOS ARM64 本机  
> 不在本轮：P3 全异步数据面重写、跨平台运行验证、额外长稳实验

## 1. 结果摘要

本轮按原 P0/P1/P2/P4 方案完成转发路径和工程入口收敛，外部 CLI、JSON、
规则 DSL、控制 API 与 trace 合同保持不变。

| 维度 | 实施前 | 实施后 |
|---|---:|---:|
| Rust 总行数 | 52,195 | 51,401 |
| 最大 Rust 文件 | 484 行 | 421 行 |
| 参数数量 Clippy 豁免 | 39 处 / 22 文件 | 0 |
| `proxy/forward.rs` | 365 行 | 177 行 |
| H1 顶层实现 | 3 条 | 1 个 `h1_forward` 边界 |
| H2 调度入口 | 6 个组合入口 | 1 个 `dispatch` + opaque connector |
| 活文档 | 6,494+ 行 | 2,532 行 |
| Whistle 运行时 checkout | 根目录完整仓库 | 无，仅 75 文件证据快照 |

Rust 净减少 794 行。实际收益低于原始行数估算，原因是本轮没有用删除
参数来隐藏状态，而是新增了 `ForwardCtx`、响应上下文、连接输入对象和组合
决策测试，并为保留的手写 H1 增加了独立流式 body source。重复协议栈已经
删除，模块边界和 Clippy 合同同时收紧。

## 2. 当前统一基线

本机开发门禁：

```sh
cargo build --workspace --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/check.sh all
```

按需验收入口：

```sh
./scripts/verify.sh <actions|matrix|bench|coverage|perf|soak|package|stream>
./scripts/targets.sh <criterion|e2e|soak|coverage|regression> REPORT...
```

本轮只执行本机和直接受影响的验证，不执行 Linux/Windows target-OS 或交叉
编译验证，也不重复 90 分钟 soak。

## 3. P0：H1 决策基准

正式历史基线为本机 45,392 rps。为选择 H1 实现，补充了同规格短对照：

| 路径 | 请求数 / 并发 | Proxy rps | p50 |
|---|---:|---:|---:|
| 当前手写快路径 | 10,000 / 32 | 36,078.44 | 688.25 us |
| 临时禁用快路径 | 10,000 / 32 | 31,254.80 | 920.96 us |

关闭手写 H1 后吞吐下降 13.37%，因此采用原方案的反向分支：保留同步手写
H1，删除 Hyper H1。原始 JSON 已归档在
`docs/archive/evidence/simplify-baseline/`。

## 4. P1：内部结构整理

### P1.1 上下文对象

已完成：

- `proxy/forward/context.rs` 定义 `ForwardInput` 与 `ForwardCtx`。
- request/response/CONNECT/H2/TLS 状态改为命名输入对象，不再跨层透传
  8-24 个位置参数。
- 删除全部 39 个 `too_many_arguments` 局部豁免。
- README 与 CI 的 Clippy 命令删除全局 `-A`，workspace 严格 Clippy 通过。

### P1.2 连接池公共核心

`upstream_pool.rs` 统一按 key 的 active slot、等待、超时、释放和错误文本。
H1 保留线程本地空闲连接缓存，H2 保留 stream lease、connector generation
和 multiplexed sender；两种不同语义没有被强行合并。

### P1.3 消息层

删除 Hyper H1 后不再存在双份 message codec。保留的一份提升为顶层
`upstream_message.rs`，由 `upstream_h2` 使用；request/header/trailer 限制
仍由原测试覆盖。

### P1.4 文件边界

选择保守方案：继续保持 500 行硬上限，不放宽到 800。只按职责调整文件，
没有为了减少文件数重新制造大文件。当前最大文件为 421 行，所有测试继续
位于 `src/**/tests/` 或 crate `tests/` 专用路径。

## 5. P2：转发路径收敛

### P2.1 H1 与 WebSocket

- 删除 `src/upstream_h1.rs` 和 `src/upstream_h1/` Hyper 实现及其专属测试。
- 原快路径与普通 `Connection: close` 兼容路径统一进入
  `proxy/h1_forward/`；`forward.rs` 只看到一个 H1 owner。
- `h1_forward/body_stream.rs` 将 fixed/chunked/close-delimited H1 body 接入
  protocol-neutral streaming response，H2→H1 bridge 保持首块下发与有界回压。
- `proxy/websocket_forward.rs` 独立负责 upgrade 响应和双向隧道收尾。
- 保留 `h1-fast-path`、pool hit/miss、超时文本和 trace flags 外部合同。

当前 H1 源码边界（含 pool/response/fallback/body stream，不含测试）为
1,386 行；实施前
`fast_h1 + manual_h1 + Hyper upstream_h1` 共约 2,192 行。

### P2.2 H2 单入口

调用方只使用：

```rust
upstream_h2::dispatch(H2DispatchRequest { ... })
```

返回值统一为 `Response | Streaming | Connect(H2Connector)`。pool hit、等待、
stale sender 重试和新连接语义留在模块内部；connector 持有 lease、request、
body mode 和 limits，`forward.rs` 不再操作内部 lease。

### P2.3 显式路由决策

`proxy/forward/plan.rs` 定义：

```rust
enum UpstreamPlan {
    H1 { pooled: bool },
    H2 { pooled: bool, streaming: bool },
    WebSocket,
}
```

协议、池资格、流式上传和 WebSocket 优先级由纯决策函数选择。专用
`forward/plan/tests.rs` 覆盖全部 16 种布尔组合。`forward.rs` 从 365 行降为
177 行，不再按 4 条 H1 和 6 条 H2 API 逐项试探。

## 6. P3：运行模型

未执行，仍按原决策保留同步数据面 + Tokio/Hyper bridge。全异步化会同时
触及 I/O、deadline、trace、隧道和 WebSocket，不属于本轮结构优化。

仅在出现以下条件时另立项目：

- thread-per-connection 无法满足明确并发目标；
- 同步/异步 bridge 出现可复现死锁或饥饿；
- P2 收敛后仍有无法局部修复的双运行时问题。

## 7. P4：工程设施

### P4.1 三个脚本入口

- `scripts/check.sh`：`lines/layout/whistle/workflows/all`
- `scripts/verify.sh`：行为合同、质量合同、资源测试、coverage/fuzz/npm-Bun package
- `scripts/targets.sh`：criterion/e2e/soak/coverage/regression 报告断言
- `scripts/lib.sh`：repo root 与任务分发公共函数

具体实现位于 `scripts/tasks/`。原 20 个文件名现在只是兼容 wrapper，可在
一个发布周期后删除；README 与 workflows 只引用三个稳定入口。

### P4.2 活文档与归档

一次性资格报告和 evidence 已移到 `docs/archive/`。活文档保留 architecture、
configuration、testing、rules DSL、technical design 和本实施记录。README
不再把历史审计列为必读设计文档。

### P4.3 Whistle 夹具

夹具已经是按合同反推的最小集合：72 个 migration source + 3 个 option 文档，
共 75 个原始证据文件；额外四个文件是 README、snapshot metadata、license
和 hashes。没有可安全继续删除的未引用上游文件。

### P4.4 元检查

保留 workflow 静态检查，但降为 `scripts/check.sh workflows` 子命令；Rust
行数上限仍为 500。CI 调用 `scripts/check.sh all`，不再展开多个根脚本。

## 8. 最终目录边界

```text
crates/rsproxy-cli/src/
  proxy/forward/             ForwardCtx、UpstreamPlan、streaming H2 收尾
  proxy/h1_forward/          唯一 H1 owner、pool、framing、fallback
  proxy/websocket_forward.rs WebSocket upgrade/tunnel 收尾
  proxy/upstream_response/   protocol-neutral buffered/streaming response
  upstream_h2/               H2 pool、connection、request body、streaming
  upstream_message.rs        Hyper H2 message codec
  upstream_pool.rs           共享 keyed admission
scripts/
  check.sh verify.sh targets.sh lib.sh
  tasks/                     具体任务实现
docs/
  archive/                   历史资格报告与 evidence
```

## 9. 回归边界

本轮不改变外部合同。重点回归 owner 为：

- `proxy/tests/h1_forward.rs`
- `proxy/tests/protocol_matrix/`
- `proxy/tests/websocket_nonblocking.rs`
- `upstream_h2/tests/`
- `scripts/verify.sh actions`
- `scripts/verify.sh matrix`

## 10. 本机最终验证

2026-07-12 在当前 Apple M1 Pro / macOS ARM64 本机完成：

- `cargo test --workspace --all-targets --no-fail-fast --locked`：通过；
  `rsproxy` 292 个单元测试通过，1 个 1 GiB 资源测试按声明保持 ignored。
- `cargo clippy --workspace --all-targets --locked -- -D warnings`：通过。
- `./scripts/check.sh all`：500 行、测试目录、Whistle 隔离和 workflow 合同通过。
- `./scripts/verify.sh actions`：17 个行为 owner 与 3 个规则/迁移合同通过。
- `./scripts/verify.sh matrix`：34 个本机协议矩阵 case 通过。
- H1 选择短对照完成；未重复跨平台、90 分钟 soak 和 1 GiB 资源实验。
- 验证结束后执行 `cargo clean`，删除 21,266 个构建文件（5.9 GiB）。

本轮 P0/P1/P2/P4 目标完成，P3 明确留作独立项目，不计入本次完成条件。

## 11. npm/Bun 分发收口

2026-07-12 在既有 Rust 拆分上新增 `packages/npm/`，不把 JavaScript 包装层混入
任何 Rust crate：

- `targets.json` 唯一定义 macOS/Linux/Windows arm64/x64 映射；Linux 分为
  glibc/musl，共 8 个原生包。
- `@rsproxy/runtime` 只负责平台/libc 选择与原生进程转发；`@rsproxy/cli` 使用
  Node shebang，`@rsproxy/bun` 使用 Bun shebang。
- 所有原生包均为 optional dependency，无 `postinstall`、无安装时 Rust 编译。
- Cargo workspace 与三个 crate 显式 `publish = false`；公开产物只进入 npm
  registry，未保留 GitHub Release、crates.io 或其他安装渠道。
- release workflow 在原生 runner 上准备 8 个平台包，再发布 runtime、npm 与
  Bun 启动器；本轮未执行这些远端 target job。

本机已用真实 `aarch64-apple-darwin` release 二进制分别完成 npm/Bun 的 pack、
install 和 `rsproxy --version`。其余平台只完成结构、映射和 workflow 适配，不记为
目标 OS 已验证。
