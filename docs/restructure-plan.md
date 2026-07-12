# rsproxy 架构改革执行方案（Breaking Restructure Plan）

> 状态：待执行（全部决策点已裁定，见 §5 决策记录）
> 性质：破坏性重构 —— 内部 API、crate 边界、错误模型、CLI 解析层、工程脚本全部重做
> 不变式：对外合同冻结 —— CLI JSON schema（`rsproxy.cli.*/v1`）、规则 DSL corpus、控制 API、trace 数据合同、npm 包名与安装体验
> 基线：51,391 行 Rust；`rsproxy-cli` crate 36,306 行（生产代码 23,731 行）；本机 H1 基准 45,392 rps

---

## 0. 诊断：现状与最佳实践的差距

先说结论：这个仓库的**文档纪律、测试分层、npm 分发模型已经高于行业平均水平**，不需要推倒。真正偏离 Rust CLI 工程最佳实践、且值得一次性破坏性解决的，是以下六项结构性债务：

| # | 问题 | 现状 | 最佳实践 |
|---|------|------|----------|
| D1 | **单体二进制 crate** | `rsproxy-cli` 一个 crate 承载 CLI、守护进程、代理数据面、控制 API、HTTP/1+2 wire 层、DNS、TLS/MITM、CA 管理、系统代理、TUI、JSON 导出，共 24k 行生产代码。模块边界只靠 `pub(crate)` 和 review 纪律维持 | 薄二进制 + 分层库 crate。边界由编译器强制；增量编译和测试并行度大幅提升；`README` 声称 "split by domain rather than by deployment unit"，但 80% 的 domain 都挤在一个 deployment unit 里 |
| D2 | **无 workspace 级依赖/lint/profile 治理** | 根 `Cargo.toml` 仅 15 行；`bytes`/`serde`/`criterion` 等版本散落在各 crate 手工重复；Clippy 靠 CI 命令行 `-D warnings`；无 release profile 调优 | `[workspace.dependencies]` 单源版本、`[workspace.lints]` 进 manifest、`[profile.release]` 做 LTO/strip —— 对 npm 分发的二进制体积是直接收益 |
| D3 | **手写 CLI 解析** | `cli/args.rs` 用位置扫描实现 `option_value`/`has_flag`；`cli/help.rs` 手写全部 usage 文本；`cli/completions.rs` 手写补全脚本；`cli/mod.rs` 用 11 个 `use xxx::*` glob 拼装。未知参数不报错、拼写错误静默忽略、help/实现可漂移 | `clap`（derive）+ `clap_complete`。类型化参数、自动 help、自动补全、拼写建议，删除约 1,500 行手写解析/帮助/补全代码 |
| D4 | **字符串错误模型** | `run_cli() -> Result<(), String>`；72 个文件以 `String` 作错误通道。错误无分类、无退出码语义、靠 `main.rs` 里的 `--json` 参数嗅探决定渲染格式 | 库 crate 用 `thiserror` 定义领域错误；二进制层统一映射到「人类可读 / JSON / 退出码」三种呈现。错误码进入 CLI JSON 合同 |
| D5 | **Shell 脚本承载工程门禁** | 25 个 bash 脚本（行数上限、测试布局、workflow 校验、性能目标比对）+ `check.sh`/`verify.sh`/`targets.sh` 三个调度器。POSIX-only，与 Windows CI 矩阵自相矛盾，逻辑不可单测 | `cargo xtask` 模式：门禁逻辑写成 Rust，跨平台、类型化、可测试。shell 只保留真正的进程编排（soak/e2e） |
| D6 | **版本多源** | npm `package.mjs` 用正则从 `Cargo.toml` 抠版本号；3 个 npm 包 + 8 个平台包各自硬编码 `0.1.0` | 版本单源于 workspace，`xtask release` 一条命令完成 bump + npm manifest 同步 |

**明确不动的部分**（已是最佳实践，重构反而引入风险）：

- `rsproxy-rules` 与 `rsproxy-trace` 的内部模块结构、公共 corpus、Whistle 证据快照；
- npm 分发的 **launcher + runtime + optionalDependencies 平台包** 三层模型（与 esbuild/Biome 同构）；
- `fuzz/` 独立于 workspace（cargo-fuzz 标准做法）；
- `benches/` 的 e2e/soak/criterion 编排；
- 白盒测试放 `src/<module>/tests/`、黑盒放 `tests/` 的分层原则（布局检查器换实现，原则保留）。

---

## 1. 目标架构

### 1.1 目标 workspace 布局

```text
rsproxy/
├── Cargo.toml                  # workspace.dependencies / lints / profiles / metadata
├── crates/
│   ├── rsproxy-rules/          # [不变] 规则 DSL：解析、匹配、索引、解释
│   ├── rsproxy-trace/          # [不变] 会话模型、内存存储、spill 持久化
│   ├── rsproxy-net/            # [新] 协议与 IO 原语层
│   ├── rsproxy-engine/         # [新] 代理数据面（核心域）
│   ├── rsproxy-control/        # [新] 控制 API：传输、认证、路由、资源
│   ├── rsproxy-platform/       # [新] OS 适配：CA 信任链、系统代理、进程管理
│   ├── rsproxy-cli/            # [重写] 薄二进制：命令、呈现、组合根
│   └── xtask/                  # [新] 工程自动化（替代 scripts/ 门禁类脚本）
├── packages/npm/               # [微调] 分发层结构不变，版本改为生成
├── benches/                    # [不变] e2e / criterion / soak 编排
├── fuzz/                       # [不变]
├── scripts/                    # [收缩] 仅保留进程编排型脚本，门禁全部迁入 xtask
└── docs/                       # [重写 architecture.md，按 crate 分章]
```

### 1.2 依赖方向（编译器强制，替代文档约定）

```text
rsproxy-cli ──→ rsproxy-control ──→ rsproxy-engine ──→ rsproxy-net
     │                │                   │                │
     ├──→ rsproxy-platform（无内部依赖）    │                └─ 无内部依赖
     └──→ rsproxy-rules / rsproxy-trace ←──┘（engine、control 同样依赖）
```

规则：

- `rsproxy-net` 与 `rsproxy-platform` 是叶子 crate，不得依赖任何 rsproxy 内部 crate；
- `rsproxy-engine` 不得感知 CLI 参数、控制 API 或呈现格式；
- `rsproxy-control` 依赖 engine 仅因为 replay 复用转发路径；除 replay 外只消费 trace/rules 的公共 API；
- `rsproxy-cli` 是唯一的组合根：装配配置、状态、监听器、日志；
- 循环依赖在此结构下**编译不过**，`check-whistle-isolation` 这类"靠 grep 防越界"的脚本随之删除。

### 1.3 模块 → crate 迁移映射表

现 `crates/rsproxy-cli/src/` 下每个顶层模块的去向（执行 Phase 2 时逐行核对）：

| 现模块 | 去向 | 说明 |
|--------|------|------|
| `http/`（h1 wire、body、trailers） | `rsproxy-net` | 协议原语，零业务语义 |
| `h2/`（下游 h2 admission、runtime） | `rsproxy-net` | 同上 |
| `dns.rs` | `rsproxy-net` | |
| `async_io.rs` | `rsproxy-net` | |
| `upstream_h2/`、`upstream_body.rs`、`upstream_message.rs`、`upstream_pool.rs` | `rsproxy-net` | 上游连接/池/帧传输 |
| `transfer_timing.rs`、`request_deadline.rs` | `rsproxy-net` | 计时原语被两个协议栈共享 |
| `proxy/`（全部：server、forward、h1_forward、h2_bridge、transforms、routing、tls、mitm、websocket、tunnel、mock、trace_helpers） | `rsproxy-engine` | 代理数据面整体平移，内部子结构不动 |
| `rule_store/`（组元数据、原子替换、watch、ArcSwap 快照） | `rsproxy-engine` | 运行时规则状态属于引擎 |
| `app.rs` + `app/mitm_failures.rs` | **拆两半** | `ProxyConfig`/`SharedState`/MITM 缓存 → `rsproxy-engine`（更名 `engine::state`）；CLI 覆盖、`default_storage` 等装配逻辑 → `rsproxy-cli` |
| `control/`（transport、auth、router、routes、query、replay、values） | `rsproxy-control` | |
| `windows_pipe.rs` | `rsproxy-control` | 它是控制面传输，不是通用 IO；server/client 两半都在此 |
| `cli/api.rs` + `cli/api_auth.rs`（控制面**客户端**：TCP/unix socket/named pipe 请求、token 认证） | `rsproxy-control` 的 `client` 模块 [D-18] | TUI 与全部 query 命令消费它；协议词汇与 server 同源，分开必然漂移 |
| `json/`（含 HAR 导出） | `rsproxy-control` [D-17] | **修正**：并非纯 CLI 呈现层——`control/routes/{trace,status,replay}` 直接消费，它是控制 API 的响应形状所有者；cli 经 control 公共 API 复用 |
| `cli/ca/`（证书、存储、平台信任链） | `rsproxy-platform` | 证书**生成**（`proxy/tls/certificates` 使用的部分）如与 CA 管理耦合，切分：生成留 engine，信任链安装归 platform |
| `cli/system_proxy/`（macos/linux/windows） | `rsproxy-platform` | |
| `cli/daemon/process.rs`（pidfile、跨平台 kill） | `rsproxy-platform` | daemon **编排**（绑定监听、发布 readiness）留在 cli |
| `cli/`（args、help、completions、config、rules、trace、api、daemon 编排） | `rsproxy-cli` 重写 | Phase 3 用 clap 重做 |
| `tui/` | `rsproxy-cli` | 呈现层；注意它直接调用 `cli::api::api_request` 与 `cli::args`——迁移后改走 `rsproxy_control::client`，参数解析在 Phase 3 随 clap 重做 |
| `logging.rs` | `rsproxy-cli` | 进程可观测性边界属于组合根 |
| `benchmark_support.rs`、`examples/`、`benches/certificates.rs` | 跟随其测量对象所在 crate | |

各模块自带的 `tests/` 子目录随模块平移；平移后**优先降级为新 crate 的公共 API 集成测试**（见 Phase 6）。

### 1.4 目标错误模型

```text
rsproxy-net::NetError        （thiserror；IO / 协议 / 超时分类）
rsproxy-engine::EngineError  （thiserror；#[from] NetError）
rsproxy-control::ControlError（thiserror；含 HTTP 状态映射）
rsproxy-platform::PlatformError
rsproxy-cli::CliError        （聚合层：#[from] 上述全部）
        │
        └── main.rs 单点渲染：
            human → eprintln + tracing
            --json → rsproxy.cli.error/v1（error.code 从错误枚举派生，不再固定 "command_failed"）
            退出码：2 用法错误（跟随 clap 惯例，不与工具对抗）/ 1 运行时失败 /
                    3 守护进程状态冲突（写入 JSON 合同测试）[D-02]
```

`error.code` 的丰富化是对外合同的**加法变更**（schema 仍为 v1，新增枚举值），在 `cli_json_contracts.rs` 中固化。

---

## 2. 执行计划

七个 Phase，每个 Phase 是一个独立可合并、可回滚的 PR（或 PR 组）。**顺序不可调换**：治理层先行是因为拆 crate 需要 `workspace.dependencies`；拆分先于 CLI 重写是为了让 clap 迁移发生在一个只剩 6k 行的薄 crate 上。

每个 Phase 的通用退出门禁（下文不再重复）：

```sh
cargo build --workspace --locked
cargo test --workspace --all-targets --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
./scripts/check.sh all          # Phase 5 后替换为: cargo xtask check all
```

### Phase 0 — 冻结基线（半天）

1. 打 tag：`git tag pre-restructure`。
2. 记录性能基线：`benches/e2e/benchmark.sh` + `scripts/targets.sh criterion`，结果存 `docs/archive/evidence/restructure-baseline/`。
3. 确认外部合同测试全绿并显式列名（这些测试在整个改革期间**一行不许改断言**，只许改 import 路径）：
   - `tests/cli_json_contracts.rs`、`tests/cli_product_matrix/`、`tests/cli_daemon_lifecycle.rs`
   - `rsproxy-rules/tests/corpus.rs`、`whistle_migration.rs`、`whistle_options.rs`
   - `control/tests/`（迁移后为 `rsproxy-control` 集成测试）
   - `packages/npm/tests/*.test.js`
4. 例外：`tests/cli_help.rs` 与 `tests/cli_completions.rs` 在 Phase 3 会因 clap 输出格式变化而重写——提前在 PR 描述中声明。
5. 创建 `CHANGELOG.md`（Keep a Changelog 格式），首条目登记本次改革的破坏面（D-01）。
   npm registry 已核实：`@rsproxy/cli` 返回 404，从未发布——破坏面确认无外部承受者。
6. 流程纪律：Phase 2–6 的每个 PR 必须同步更新 `docs/architecture.md` 受影响章节，
   不允许文档滞后到 Phase 7 一次性补——Phase 7 只做整合与润色 [D-19]。

### Phase 1 — Workspace 治理层（1 天）

1. 根 `Cargo.toml` 增加：
   ```toml
   [workspace.dependencies]
   # 全部三方依赖单源化：bytes、serde、serde_json、tokio、hyper、rustls、
   # tracing、criterion、toml、regex …（从三个 crate 现有清单合并）
   rsproxy-rules = { path = "crates/rsproxy-rules" }
   rsproxy-trace = { path = "crates/rsproxy-trace" }

   [workspace.lints.rust]
   unsafe_code = "deny"          # 仅 platform / control 两个 crate 级 allow，见 [D-05]

   [workspace.lints.clippy]
   all = { level = "deny", priority = -1 }

   [profile.release]
   lto = "thin"                  # 不用 fat：收益边际、构建时间翻倍 [D-09]
   codegen-units = 1
   strip = "symbols"
   ```
   > `panic = "abort"` **永不启用**（已决策，[D-04]）：数据面用 per-connection
   > `thread::spawn`（9 处）做 panic 隔离，单连接 bug 只杀死该连接线程；abort
   > 会把它升级为整个守护进程崩溃。unwind 是这里的可靠性特性，不是遗留项。
2. 三个现有 crate 的 `Cargo.toml` 改为 `x.workspace = true` 引用；每个 crate 加 `[lints] workspace = true`。
3. 包名一致性 [D-07]：`crates/rsproxy-cli` 的 package name 由 `rsproxy` 改为
   `rsproxy-cli`，同时声明 `[[bin]] name = "rsproxy"`。测试中的
   `CARGO_BIN_EXE_rsproxy` 按 bin 名生成，不受影响；`main.rs` 的
   `rsproxy::run_cli()` 改为 `rsproxy_cli::run_cli()`。
   改名后全仓 grep 旧包名的字符串引用并逐一核对：`scripts/`（coverage、verify
   的 `-p rsproxy` 类过滤器）、`.github/workflows/`、`packages/npm/scripts/package.mjs`。
4. `workspace.package` 增加 `rust-version`（当前工具链实测的 MSRV；edition 2024
   要求 ≥ 1.85）。分发型 CLI 声明 MSRV 是基线实践，CI 加一个 MSRV job 用该版本跑
   `cargo check --workspace`。
5. CI 的 clippy 命令行保持不变（manifest lints 与 `-D warnings` 叠加无冲突）。
6. 用 Phase 0 基线验证 release 产物：体积应下降（strip），吞吐不得回退 >10%（LTO 通常持平或略升）。

**回滚**：单 commit revert，无 API 影响。

### Phase 2 — 拆解单体 crate（核心工程，5–8 天）

自底向上四步，每步一个 PR，期间 `rsproxy-cli` 始终可编译可测：

**2a. 提取 `rsproxy-net`**

1. `cargo new crates/rsproxy-net --lib`，加入 workspace。
2. 按 §1.3 迁移 `http/`、`h2/`、`dns.rs`、`async_io.rs`、`upstream_*`、`transfer_timing.rs`、`request_deadline.rs`。
3. 可见性：原 `pub(crate)` 项按「engine 实际调用面」提升为 `pub`，**逐项**而非批量——`lib.rs` 做显式 re-export facade（与 rules/trace 现行风格一致）。
4. 新增 `crates/rsproxy-net/tests/public_api.rs`（照抄 rules/trace 的公共 API 快照测试模式）。
5. `rsproxy-cli` 改从 `rsproxy_net::` 引用；模块内相对路径 `crate::http::` → `rsproxy_net::http::`。

**2b. 提取 `rsproxy-engine`**

1. 迁移 `proxy/` 整体、`rule_store/`、`app.rs` 的运行时半边（`SharedState`、`MitmCertCache`、`MitmFailureCache`、`AppConfig` 更名 `ProxyConfig`）。
2. 引擎入口收敛为显式 API：`engine::serve(listener, state)`、`engine::state::SharedState::new(config)`——CLI 与 control 只能走这两个门。
3. `proxy/tests/` 的 17 个 action-effect 套件、protocol_matrix、streaming 等全部随迁；它们驱动 `handle_client` 真实路径，天然是 engine 的集成测试。
4. `scripts/verify.sh actions|matrix` 中的测试过滤器路径同步更新（34 个精确测试名清单在 `test-protocol-matrix.sh` 里）。

**2c. 提取 `rsproxy-control`**

1. 迁移 `control/` + `windows_pipe.rs` + `json/`（响应形状，[D-17]）+
   `cli/api.rs`、`cli/api_auth.rs`（重组为 `control::client`，[D-18]）。
   crate 内部按 `server / client / shapes` 三个顶层模块组织，token 认证词汇两端共享。
2. control 对 engine 的依赖面收窄成两个 trait 或显式类型：trace 查询走 `rsproxy-trace` 公共 API，replay 走 `engine::replay` 入口。
3. `control/tests/` 随迁为 `rsproxy-control` 的测试；`tui/` 与 CLI query 命令改从 `rsproxy_control::client` 引用。

**2d. 提取 `rsproxy-platform`**

1. 迁移 `cli/ca/`（信任链、存储）、`cli/system_proxy/`、`cli/daemon/process.rs`。
2. 证书切分 [D-06]：`rcgen` 叶证书生成留在 engine（MITM 热路径）；CA 根证书生成/指纹/文件状态/平台信任安装归 platform。**跨界不新建共享类型 crate，也不允许任何一方 import 另一方**——CA 材料以第三方公共词汇跨界（`rustls::pki_types::{CertificateDer, PrivateKeyDer}`、PEM/DER 字节），双方本就都依赖 rustls。组合根（cli）从 platform 读出 CA 材料，转换后填入 engine 的 `ProxyConfig` 字段。
3. `windows-sys`/`libc` 依赖跟随迁移，`rsproxy-cli` 的依赖表显著变薄。

**每步的专项验收**：除通用门禁外，跑 `./scripts/verify.sh actions && ./scripts/verify.sh matrix`（2b 后）、control 集成测试（2c 后）、`cli_daemon_lifecycle`（2d 后）。全部四步完成后重跑 Phase 0 性能基线，接受 ±10% 内波动。

**回滚**：每步独立 revert。跨 crate 移动是纯机械平移 + 可见性调整，不含行为变更——review 时用 `git log --follow` 验证文件历史连续。

### Phase 3 — CLI 层重写（3–4 天）

此时 `rsproxy-cli` 只剩：命令分发、config、json/、tui/、logging、daemon 编排，约 6–7k 行。

1. 引入 `clap`（derive + `wrap_help`）与 `clap_complete`：
   - 每个子命令一个 `#[derive(Args)]` 结构体，替换 `option_value` 扫描；
   - 多值参数等价性 [D-14]：现有多值参数仅三个——`--dns-server`、
     `-H/--header`、`--response-header`，全部映射为
     `Vec<String>` + `ArgAction::Append`；删旧实现前先为这三个写等价断言
     （顺序保持、重复保留）；
   - 删除 `cli/args.rs`、`cli/help.rs`、`cli/completions.rs` 手写实现（约 1,500 行）；
   - `rsproxy completions zsh|bash|fish|powershell` 改由 `clap_complete` 生成。
2. 错误模型落地（§1.4）：`thiserror` 枚举替换 `Result<_, String>`，从 cli crate 开始向下逐 crate 替换（net/engine/control/platform 各自的错误枚举在 Phase 2 迁移时先以 `String` 原样平移，本 Phase 统一收割）。
3. `main.rs` 重写为唯一呈现点：human/JSON/退出码三路输出。`--json` 的 argv
   嗅探**仅保留一条路径** [D-03]：clap 解析失败时拿不到类型化参数，此时嗅探
   argv 决定 usage 错误的渲染格式；解析成功后的一切错误一律走类型化字段。
4. 重写 `tests/cli_help.rs`、`tests/cli_completions.rs` 以匹配 clap 输出；**JSON 合同测试必须原样通过**（新增 `error.code` 值除外，作为加法断言追加）。
5. 移除 `cli/mod.rs` 的 glob re-export，改为显式 `use`。

**破坏面声明**：`--help` 文本格式、错误消息措辞、未知参数从"静默忽略"变为"报错退出"。这是行为修正而非回归——但若有外部脚本依赖旧宽容行为，会在此处断裂。

### Phase 4 — 版本与发布单源化（1 天）

1. `xtask release <version>`（xtask 骨架在此 Phase 先建，门禁迁移在 Phase 5）：
   - 更新 `workspace.package.version`；
   - 重写 `packages/npm/{cli,bun,runtime}/package.json` 与 `targets.json` 派生的 8 个平台包 manifest 的 version 及互相引用；
   - `package.mjs` 删除正则解析 Cargo.toml 的 `workspaceVersion()`，改读 `cargo metadata --format-version 1`。
2. `release.yml` 的 npm publish 增加 `--provenance`（GitHub OIDC，分发型 CLI 的当前最佳实践）。

### Phase 5 — 工程自动化迁移：scripts → xtask（2–3 天）

1. `crates/xtask` 作为普通 workspace 成员（进默认构建与测试，构建秒级 [D-08]）+ `.cargo/config.toml`：
   ```toml
   [alias]
   xtask = "run --package xtask --"
   ```
2. 迁移映射：
   | 现脚本 | 去向 |
   |--------|------|
   | `check-rust-lines.sh`（500 行上限） | `xtask check lines`（限值与豁免清单进 `xtask.toml`） |
   | `check-test-layout.sh` | `xtask check layout` |
   | `check-workflows.sh` | `xtask check workflows` |
   | `check-whistle-isolation.sh` | **删除**（crate 边界已由编译器保证；fixtures 隔离改为 layout 检查的一条规则） |
   | `check-*-targets.sh` / `check-performance-regression.sh`（JSON 报告比对） | `xtask targets <kind> <report>`（serde 强类型解析，替代 jq/awk） |
   | `check.sh` / `targets.sh` 调度器 | 删除，入口即 `cargo xtask` |
   | `verify.sh` 及 `test-*.sh`、`benches/**/*.sh`、`coverage.sh` | **保留 shell**（进程编排、curl、长稳实验属于 shell 的合理领地），仅更新内部对 check.sh 的引用 |
3. CI 四个 workflow 中 `./scripts/check.sh …` 替换为 `cargo xtask check …`；Windows job 从此也能跑门禁（消除 D5 的平台矛盾）。
4. 追加 `cargo deny check`（licenses/advisories/bans）进 CI——二进制分发工具的供应链底线。
5. 根 `package.json` 的 `check:packages` 保留（node --test 是 npm 层的原生归属）。

### Phase 6 — 测试布局收敛（与 Phase 2–3 并行推进，收尾 1 天）

1. 原则不变：白盒 `src/<module>/tests/`、黑盒 `tests/`。
2. 拆分红利：Phase 2 迁移的白盒测试中，凡只消费新 crate 公共 API 的，降级为该 crate `tests/` 下的集成测试（编译单元独立、并行度更高）；确需私有访问的保留白盒位置。以 `proxy/tests/` 为主要收割区。
3. 每个新 crate 补 `tests/public_api.rs` 快照。
4. `xtask check layout` 校验新五 crate 布局。

### Phase 7 — 文档重写与收尾（1 天）

1. `docs/architecture.md` 整合润色（各章已随 Phase 2–6 逐 PR 更新，见 [D-19]）：统一为按 crate 分章，每章 = 边界声明 + 公共 API 面 + 内部子结构；§1.2 依赖图入文。
2. `README.md` 更新 workspace 树与构建命令（`cargo xtask check all`）。
3. `docs/simplification-plan.md`、本文档移入 `docs/archive/`。
4. 首次 `xtask release 0.2.0` [D-11]：改革完成即 bump 次版本号，标记 CLI 表面变更。
5. 遗留项登记（不在本轮）：`proxy/` 内部进一步子域化（transforms 与 forward 的耦合）、cargo publish 到 crates.io 的可行性评估。（`panic = "abort"` 已从遗留项移除——[D-04] 决策为永不启用。）

---

## 3. 风险登记与对策

| 风险 | 等级 | 对策 |
|------|------|------|
| crate 拆分放大公共 API 面，内部类型被迫 `pub` | 高 | 每 crate 强制 facade + `public_api.rs` 快照测试；review 检查每个新 `pub` 是否有跨 crate 调用者；宁可 re-export 精确条目也不 `pub mod` 整包 |
| 性能回退（跨 crate 内联失效） | 中 | Phase 1 已开 thin-LTO，跨 crate 内联在 release 下恢复；每 Phase 结束比对 criterion ±10% 门禁；热路径（h1_forward、transforms）在 2a/2b 后立即跑 `benchmark.sh` |
| `app.rs` SharedState 拆分引出隐藏循环依赖（engine 需要回调 CLI 层能力） | 中 | 出现即用「组合根注入」解决：engine 定义 trait，cli 实现并传入；禁止 engine 反向依赖 |
| clap 迁移遗漏某个手写参数的边角语义（如重复参数取值顺序） | 中 | Phase 3 前用 `cli_product_matrix` 离线套件全量过一遍；对 `option_values`（多值参数）逐个写等价断言再删旧实现 |
| 外部脚本依赖旧的宽容参数解析 | 低 | 破坏面已在 Phase 3 声明；CHANGELOG 标注 |
| 迁移期间 main 分支长期不绿 | 低 | 每 Phase 独立 PR + 通用门禁；Phase 2 内部四步也各自可合并 |

## 4. 工作量与顺序总览

```text
Phase 0  冻结基线            0.5 天   ──┐
Phase 1  workspace 治理      1 天       │ 第 1 周
Phase 2  拆解单体（4 PR）    5–8 天   ──┤
Phase 3  CLI 重写            3–4 天     │ 第 2 周
Phase 4  版本单源化          1 天     ──┤
Phase 5  scripts → xtask     2–3 天     │ 第 3 周
Phase 6  测试收敛            并行+1 天  │
Phase 7  文档收尾            1 天     ──┘
                             合计 ≈ 15–20 个工作日
```

## 5. 决策记录（ADR）

全部悬决点已裁定。执行期间如需推翻某条，必须在本表追加"推翻原因"，不允许静默偏离。

| # | 决策点 | 裁定 | 依据 |
|---|--------|------|------|
| D-01 | Phase 3 破坏面（help 文本变化、未知参数从静默忽略变报错、`cli_help`/`cli_completions` 测试重写）是否需要兼容垫片 | **全量接受，不做垫片** | **已验证**：npm registry 对 `@rsproxy/cli` 返回 404（从未发布）；`publish = false`；资格范围仅本机 macOS ARM64。不存在外部承受者，宽容解析是缺陷不是特性。CHANGELOG 标注即可 |
| D-02 | 退出码分配 | **2 = 用法错误，1 = 运行时失败，3 = daemon 状态冲突** | clap 对 usage 错误默认退出码为 2（亦是 bash misuse 惯例）；跟随工具与生态，不自定义再去覆写 clap 行为 |
| D-03 | `--json` argv 嗅探去留 | **仅保留 clap 解析失败这一条路径**，其余全部走类型化字段 | 解析失败时类型化参数不存在，嗅探是该路径的唯一可行方案；其他路径保留嗅探就是保留隐患 |
| D-04 | `panic = "abort"` | **永不启用**，从遗留项移除 | 数据面 9 处 per-connection `thread::spawn`，panic 隔离在单连接线程内是可靠性特性；abort 将单连接 bug 升级为整个 daemon 崩溃，对代理这种长驻进程是净损失 |
| D-05 | `unsafe_code` lint 粒度 | **workspace 级 deny；仅 `rsproxy-platform`（daemon/process、system_proxy/windows）与 `rsproxy-control`（windows_pipe）crate 级 allow 并写明理由** | 现有 unsafe 仅 4 个文件，全部落在这两个 crate 的迁移目的地；`app.rs` 的 unsafe 随状态拆分归入 platform 侧装配代码。net/engine/cli 保持零 unsafe，由编译器守住 |
| D-06 | 证书类型如何跨 engine/platform 边界（原稿自相矛盾：类型放 platform 但 engine 不 import platform） | **以第三方公共类型跨界**（`rustls::pki_types`、PEM/DER 字节），不新建共享 crate，组合根负责转换注入 | 双方本就依赖 rustls，第三方词汇是零成本公共语言；为两个字段建共享 crate 是过度设计 |
| D-07 | crate 目录 `rsproxy-cli` 与包名 `rsproxy` 不一致 | **包名改 `rsproxy-cli`，`[[bin]] name = "rsproxy"`**，纳入 Phase 1 | `CARGO_BIN_EXE_rsproxy` 按 bin 名生成、测试零改动；命令名（用户所见）不变 |
| D-08 | xtask 是否进默认 workspace 成员 | **进** | 构建秒级；排除出去反而需要维护 default-members 清单 |
| D-09 | LTO 档位 | **thin，不用 fat** | thin-LTO 已恢复跨 crate 内联（拆分风险的主对策）；fat 构建时间约翻倍、收益边际。若 Phase 2 后 criterion 门禁不过，再升 fat 作为对策，不预付成本 |
| D-10 | 500 行文件上限是否放宽 | **保留 500 硬上限，仅换 xtask 实现** | 当前最大文件 421 行，纪律已内化且无痛；放宽是纯风险无收益 |
| D-11 | 改革后版本号 | **0.2.0**（`xtask release` 首跑） | CLI 表面行为变更（D-01）按 semver 0.x 惯例 bump minor |
| D-12 | `packages/npm` 是否重命名/重组 | **不动** | 该层已是 esbuild/Biome 同构的最佳实践；重命名是纯 churn |
| D-13 | crate 数量（net 是否并入 engine） | **维持 8 crate，net 独立** | net 无内部依赖、24k 行中约 6k 行且独立可测，是编译并行度和边界强制的关键切口；并入 engine 则 D1 只解决一半 |
| D-14 | clap 多值参数等价性风险 | **逐个写等价断言再删旧实现**；清单已穷尽：`--dns-server`、`-H/--header`、`--response-header` 三个 | 已 grep 确认 `option_values` 全部调用点，不存在第四个多值参数 |
| D-15 | 白盒测试降级为集成测试的判据 | **只降级"仅消费新 crate 公共 API 且不依赖测试内部构造器"的套件；存疑一律保留白盒** | 降级是编译并行度优化，不值得为它扩大公共 API 面（与 D-06 同理：API 面最小化优先） |
| D-16 | engine 需要 CLI 侧能力时的通信模式 | **组合根注入：engine 定义 trait，cli 实现并传入**；禁止任何反向依赖 | 唯一不引入循环依赖的模式；trait 数量预期 ≤ 2（若超出，说明切分线画错，回到 §1.3 重审） |
| D-17 | `json/`（含 HAR）归属 | **`rsproxy-control`，不是 cli** | 初稿有误：grep 证实 `control/routes/{trace,status,replay}` 直接消费 `crate::json`——它是控制 API 的响应形状所有者，放 cli 会让 control 编译失败；cli 的 human 呈现经 control 公共 API 复用 |
| D-18 | 控制面客户端（`cli/api.rs`、`api_auth.rs`）归属 | **`rsproxy-control::client` 模块**，与 server 同 crate | 初稿遗漏该组件。TUI 与全部 query 命令依赖它；client/server 共享传输与 token 词汇（含 windows_pipe 两半），拆到不同 crate 必然协议漂移 |
| D-19 | `docs/architecture.md` 更新时机 | **每个 Phase 的 PR 同步更新受影响章节**；Phase 7 只做整合润色 | 该文档是仓库的活设计文件（README 要求改跨模块行为前必读），滞后 4 个 Phase 等于让所有中间 PR 的 reviewer 拿着错误地图 |

改革完成后的验收快照（写进最终 PR 描述）：

- workspace 成员：8 个 crate（rules / trace / net / engine / control / platform / cli / xtask）；
- `rsproxy-cli` 生产代码从 23,731 行降至 ≈ 5,000 行；
- 手写解析/帮助/补全 ≈ 1,500 行删除，`Result<_, String>` 清零；
- 门禁脚本从 25 个收缩到 ≤ 10 个（仅进程编排类）；
- 外部合同零破坏：JSON schema、DSL corpus、控制 API、npm 安装体验全部原样；
- 性能：criterion 与 e2e 基准相对 `pre-restructure` tag 在 ±10% 内。
