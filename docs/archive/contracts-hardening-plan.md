# rsproxy 契约硬化执行方案（Contracts Hardening Plan）

> 状态：已完成（起草并执行于 2026-07-13）
> 性质：非破坏性 —— 不改 crate 边界、不改运行时行为、不改对外合同；改的是「层间契约的可见性与可执行性」
> 前置：架构改革（`docs/archive/restructure-plan.md`，D-01…D-19）已完成并合入 `main`
> 基线：61,766 行 Rust / 8 crate / 543 测试 / 33 个集成测试二进制 / 生产代码 50 处 `unwrap`·`expect` / 库 crate rustdoc 覆盖率接近 0

执行结果见 [`evidence/hardening-final/README.md`](evidence/hardening-final/README.md)：
7 个产品库的公开 Rust API 已文档化并生成快照，workspace lint、rustdoc、API、
layout 与 workflow 门禁全部生效；产品集成测试二进制由 33 个降至 14 个，
隔离冷测试由 99.09 秒降至 77.53 秒。所有完成定义均已验证。

---

## 0. 诊断：重构之后剩下什么

上一轮改革解决的是**结构**问题：边界由编译器强制、错误类型化、门禁进 xtask。全局复查确认宏观结构已无债务。剩余的差距全部集中在一个主题上——**8 个 crate 之间的契约存在，但不可见、不可审、不成文**：

| # | 问题 | 现状 | 最佳实践 |
|---|------|------|----------|
| H1 | **公共 API 零文档** | `rsproxy-trace` 0 行 `///` 对 ~47 个 pub 项；`rsproxy-rules` 6 行对 ~90 项；全 workspace 库 crate 合计约 150 行文档对 ~370 个 pub 项。拆 crate 后每个 pub 面就是层间契约，契约是裸的 | `missing_docs = "deny"` + `cargo doc -D warnings` 进 CI。每个 pub 项至少一句话说明「它承诺什么」 |
| H2 | **rules crate API 面失控** | `lib.rs` 中 `pub use action::*; pub use model::*;`——往这两个文件加任何 pub 类型即静默成为跨 crate 契约，无人审查。其余 7 个 crate 均为显式导出，仅此一处例外 | 显式导出清单（约 35 个名字），glob 归零 |
| H3 | **Lint 姿态低于工程水位** | 仅 `clippy::all = deny` + `unsafe_code = deny`。`unreachable_pub`、`unwrap_used`、`dbg_macro` 等对多 crate workspace 高价值的 lint 未启用 | 精选（curated）deny 清单，而非盲开 pedantic；测试代码经 `clippy.toml` 豁免 |
| H4 | **panic 政策不成文** | 生产代码 50 处 unwrap/expect，同为锁中毒场景风格分裂：`mitm.rs:50` 写 `.lock().unwrap()`，`rule_store.rs:100` 写 `.expect("rule store poisoned")`。D-04 已把 panic 隔离定为特性，但「何时允许 panic、怎么写」无规则 | `clippy::unwrap_used = deny`；允许 `expect`，且 message 必须陈述被违反的不变量 |
| H5 | **Rust API 无快照合同** | CLI JSON 有 `cli_json_contracts` 锁 schema，规则 DSL 有 corpus，**唯独 crate 间的 Rust API 没有等价物**。现有 `tests/public_api.rs` 是行为冒烟测试（保留），不能发现「API 面悄悄变大/变小」 | `cargo public-api` 快照进仓库，PR 中 API 变更必须以 diff 形式可见，与 ADR 纪律同构 |
| H6 | **组合根存在 Deref 反模式** | `AppConfig` 通过 `Deref/DerefMut<Target = ProxyConfig>` 透传引擎配置，cli 内约 81 个调用点隐式跨层取字段。Deref 到非指针类型是 API Guidelines 明确劝阻的写法，且恰好模糊了刚建立的 cli/engine 边界 | 删 Deref，显式 `engine()` 访问器；调用点一眼可见「这是引擎字段」 |
| H7 | **集成测试二进制过多** | 33 个 `tests/*.rs` 独立二进制（rules 9、cli 9、net 6、platform 4、engine 3、control 1、trace 1），每个都是独立链接单元，是冷 `cargo test` 链接耗时大头 | matklad one-binary 模式：纯逻辑类合并为每 crate 一个 harness；进程级 e2e 保持独立 |
| H8 | **重构残留** | 12 个空目录（`cli/src/dns`、`cli/src/h2`、各 crate 空 `src/error/` 等，git 不追踪但误导读者）；8 个 crate 全部缺 `description` 元数据 | 清理 + xtask layout 门禁拒绝空目录，杜绝复发 |

**明确不动的部分**（复查后确认不是债务）：

- `benches/` + `scripts/` 剩余约 1,500 行 shell：分工正确——shell 驱动外部进程（oha、whistle、npm），xtask 校验报告 JSON。`verify.sh → scripts/tasks/` 的两层薄壳可议但不值得动；
- `rsproxy-cli` 直依赖 `rsproxy-engine`：仅组合根用 `CaMaterial`/`ProxyConfig` 装配（`app.rs`），正是 D-06 设计的注入点；
- 各 crate 的 `tests/public_api.rs` 行为冒烟测试：与 H5 的快照互补（一个测「能不能用」，一个测「面有没有变」），保留；
- D-04 panic 隔离政策本身：H4 只统一书写风格，不改变「锁中毒即 panic、由连接线程吸收」的设计；
- npm 三层分发模型、fuzz 独立 workspace、白盒/黑盒测试分层原则。

---

## 1. 目标状态

### 1.1 完成后的 workspace 治理面

```toml
# Cargo.toml（目标增量，示意）
[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "deny"            # H1；xtask 以 #![allow(missing_docs)] 豁免
unreachable_pub = "deny"         # H3：私有模块里的裸 pub 全部现形
unused_qualifications = "deny"

[workspace.lints.clippy]
all = { level = "deny", priority = -1 }
unwrap_used = "deny"             # H4：生产代码禁 unwrap，expect 必须带不变量
dbg_macro = "deny"
todo = "deny"
unimplemented = "deny"
```

```toml
# clippy.toml（新增）
allow-unwrap-in-tests = true
allow-expect-in-tests = true
```

### 1.2 完成后的合同层次

| 合同 | 载体 | 门禁 |
|------|------|------|
| CLI JSON schema | `cli_json_contracts.rs`（已有） | `cargo test` |
| 规则 DSL 语义 | corpus + whistle fixtures（已有） | `cargo test` + xtask whistle |
| **crate 间 Rust API 面** | `crates/<c>/api.txt` 快照（**新增**） | `cargo xtask check api`（**新增**） |
| **每个 pub 项的语义** | rustdoc（**新增**） | `missing_docs = deny` + `cargo doc -D warnings` |
| 布局与行数 | xtask lines/layout（已有，layout 增加空目录检查） | `cargo xtask check all` |

### 1.3 不变式（全程冻结）

- 对外合同零变化：CLI JSON schema、规则 DSL、控制 API、trace 数据合同、npm 安装体验；
- 运行时行为零变化：H4/H6 是纯书写等价变换（`unwrap → expect`、`Deref → 显式访问器`），性能与 panic 语义完全不变；
- `cargo test` 全绿、clippy 零警告、xtask 全门禁通过是每个 Phase 的合入条件；
- 除 Phase 6 外任何 Phase 不移动文件（Phase 6 仅移动 `tests/` 下的黑盒测试）。

---

## 2. 执行方案（7 个 Phase，每个 = 1 个 PR）

排序原则：先清障（P1），再定 API 面（P2），再上强制力（P3），再填内容（P4），面稳定后拍快照（P5），最后做与合同无关的构建优化（P6）。前项均为后项减少返工。

### Phase 0 — 基线证据（并入 P1 的 PR，不单独提交)

记录到 `docs/archive/evidence/hardening-baseline/README.md`：

- `cargo build --workspace --timings` 与冷 `cargo test --workspace` 墙钟时间（P6 的对照组）；
- 每 crate rustdoc 覆盖数（`grep -rc '^\s*///'` 对 pub 项数）；
- `grep` 统计的 unwrap/expect 点位清单（P3 的核对清单）；
- 33 个测试二进制及各自测试数。

### Phase 1 — 卫生（H8）

1. 删除 12 个空目录：`cli/src/{dns,h2,cli/daemon,cli/system_proxy}`、`cli/{examples,benches}`、`{net,engine,platform}/src/error`、`platform/src/process`、`net/src/{dns,request_deadline}/tests` 等（执行时以 `find crates -type d -empty` 现场输出为准）；
2. xtask `check::layout` 增加规则：`crates/**` 下出现空目录即 fail，附带修复提示；
3. 8 个 crate 的 `Cargo.toml` 补 `description`（一句话，与未来 crates.io 兼容；`publish = false` 不变）。

**验收**：`find crates -type d -empty` 输出为空；`cargo xtask check layout` 含空目录检查且通过。

### Phase 2 — API 面定形（H2 + H6）

1. **de-glob**（H2）：`rsproxy-rules/src/lib.rs` 的 `pub use action::*; pub use model::*;` 替换为显式清单。已知面：model.rs 15 个类型（`RuleSet`、`Rule`、`Matcher`、`Condition`、`RequestMeta`、`ResponseMeta`、`ResolveResult` 等）+ action.rs 18 个类型（`Action`、`HeaderOp`、`Phase` 等）+ 少量 pub fn。以「下游 crate 编译通过所需的最小集合」为准，编译器会精确告知遗漏；导出后若有 pub 项不再被导出且无内部使用，降为 `pub(crate)`；
2. **de-Deref**（H6）：删除 `AppConfig` 的 `Deref/DerefMut` 实现，新增 `pub fn engine(&self) -> &ProxyConfig` / `engine_mut()`；cli 内约 81 个隐式调用点改为显式（纯机械，编译器驱动）；`app.rs` 自身方法内的 `self.storage` 等同步改写；
3. 顺带将 `unreachable_pub = "warn"` 加入 workspace lints（本 Phase 只观察输出、修显而易见的，deny 在 P3 落锤）。

**验收**：`grep -rn "pub use .*::\*" crates/*/src/lib.rs` 为空；`grep -rn "impl Deref" crates/rsproxy-cli/src` 为空；全门禁绿。

### Phase 3 — Lint 升档 + panic 政策成文（H3 + H4）

1. workspace lints 按 §1.1 全量落地（`missing_docs` 除外，留给 P4）；新增根 `clippy.toml` 豁免测试代码；
2. 清理 50 处点位：生产代码 `unwrap()` 全部改为 `expect("<不变量陈述>")`，锁中毒统一为 `expect("<资源名> lock poisoned")`；`tests/support` 等辅助代码若 clippy.toml 豁免不覆盖，文件头 `#![allow(clippy::unwrap_used)]` 显式标注；
3. `unreachable_pub` 从 warn 提到 deny，P2 观察到的裸 pub 全部降为 `pub(crate)` 或收进导出清单；
4. panic 政策写入 `docs/architecture.md`（一段即可）：什么情况允许 panic（锁中毒、已验证不变量）、必须用 expect、message 怎么写，并援引 D-04。

**验收**：`grep -rn "unwrap()" crates/*/src --include='*.rs' | grep -v tests` 为空；clippy 全绿。

### Phase 4 — 文档战役（H1，工作量大头）

按依赖序逐 crate 燃尽，每完成一个 crate 在其 `lib.rs` 加 `#![deny(missing_docs)]` 锁住成果（细粒度递进，避免一把梭）：

| 顺序 | crate | 规模 | 重点 |
|------|-------|------|------|
| 1 | rsproxy-rules | ~90 pub 项（含 `Action` 等大枚举的变体） | DSL 语义是全仓最需要文档的部分；变体文档可从 `docs/rules-dsl-spec.md` 提炼，反向核对 spec 与实现 |
| 2 | rsproxy-trace | ~47 项 | 内存/spill 预算类常量必须写清单位与默认值 |
| 3 | rsproxy-net | ~94 项 | 每个超时/限额参数写明「谁计时、从哪起算」 |
| 4 | rsproxy-platform | ~61 项 | 每个函数写明触碰的 OS 资源与权限要求 |
| 5 | rsproxy-engine | ~46 项 | `EngineHandle`/`SharedState` 的线程安全承诺 |
| 6 | rsproxy-control | ~17 项 | 与控制 API HTTP 面的对应关系 |
| 7 | rsproxy-cli | ~62 项 | 面向内部，允许从简 |

要求：**每项至少一句「承诺」而非复述签名**（"Returns the config" 不合格；"读取时钟一次，跨调用不缓存" 合格）。`missing_docs` 覆盖 pub 字段与枚举变体——自解释字段一句话即可，但不许空缺。

收尾动作：`missing_docs = "deny"` 移入 workspace lints，删除各 lib.rs 的临时属性；xtask 或 CI 增加 `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`（顺带锁住 intra-doc link 不烂）。

**验收**：`cargo doc` 零警告；抽查每 crate 首页（`//!`）能在 30 秒内让新人明白该 crate 的职责与禁区。

### Phase 5 — Rust API 快照门禁（H5）

1. 工具：`cargo public-api`（见 D-25，需 pinned nightly 生成 rustdoc JSON，仅 CI 工具链，产品构建仍 stable）；
2. 7 个库 crate 各生成 `crates/<c>/api.txt` 快照入仓；
3. xtask 新增 `cargo xtask check api`（对比）与 `--bless`（更新快照）；接入 `check all` 与 CI；
4. 快照更新的 PR 纪律写入 `docs/architecture.md`：API diff 必须在 PR 描述中说明动机（与 ADR override 规则同构）。

**验收**：本地改任一 pub 签名，`cargo xtask check api` 红；`--bless` 后 diff 精确呈现该变更。

### Phase 6 — 测试二进制合并（H7，独立收益，随时可做/可弃）

合并原则：**纯逻辑合并、进程级隔离保留**。

| crate | 现状 | 目标 |
|-------|------|------|
| rsproxy-rules | 9 个 | 1 个 `tests/it/main.rs`（corpus、properties、complexity 等全为纯 CPU） |
| rsproxy-net | 6 个 | 1 个 |
| rsproxy-platform | 4 个 | 1 个 |
| rsproxy-engine | 3 个 | 1 个 |
| rsproxy-cli | 9 个 | 4 个：轻量类（completions/help/logging/json_contracts/rule_groups/trace_follow）合并为 `it`；`cli_daemon_lifecycle`、`cli_product_matrix`、`large_stream_resource` **保持独立**（依赖端口/进程/大文件资源，二进制间默认串行是它们的隔离屏障） |
| control / trace | 各 1 个 | 不动 |

注意：合并后原本跨二进制串行的测试变为同二进制内并行线程——合并前逐文件检查全局资源（env var、固定端口、cwd）；有冲突的测试标注串行或留在独立二进制。

**验收**：33 → ~13 个二进制；冷 `cargo test --workspace` 墙钟对比 P0 基线，结果记入 evidence（预期链接时间可测下降；若无收益，如实记录并接受）。

---

## 3. 决策记录（延续 D-01…D-19 编号）

| # | 决策 | 理由 | 排除的替代方案 |
|---|------|------|----------------|
| D-20 | `missing_docs` 以 crate 级 `#![deny]` 逐个燃尽，全部完成后才进 workspace lints | `[lints] workspace = true` 与 per-crate lints 表互斥，属性是唯一细粒度手段；避免长期红着的分支 | 一次性 workspace deny（PR 巨大、阻塞其他工作）；长期停留在 warn（warn 无强制力，必然烂尾） |
| D-21 | rustdoc 门禁用 `RUSTDOCFLAGS="-D warnings" cargo doc --no-deps`，进 CI | 除 missing_docs 外还捕获 broken intra-doc links；成本一次 doc 构建 | 只靠 lint（抓不到坏链接） |
| D-22 | rules 导出清单以「下游编译最小集」为准，未入选 pub 项降 `pub(crate)` | API 面应由消费者需求定义，不由实现文件的 pub 关键字定义 | 照抄现有全部 pub 项（把失控面合法化） |
| D-23 | Lint 采用精选 deny 清单（§1.1），不开 `clippy::pedantic` | pedantic 含大量风格性误报，会催生 allow 泛滥反而稀释纪律；精选集每条都有明确故事 | pedantic=warn（CI 不 fail 即无效）；pedantic=deny（allow 泛滥） |
| D-24 | panic 书写规范：禁 `unwrap`，`expect` message 必须陈述不变量；测试代码经 `clippy.toml` 豁免 | D-04 已定 panic 隔离为特性，本决策只让政策可 grep、可 review；expect message 是崩溃现场的第一行线索 | 全面禁 panic 改 Result（违背 D-04，锁中毒本就该崩）；保持现状（风格分裂持续） |
| D-25 | API 快照用 `cargo public-api` + pinned nightly（仅 CI 生成 rustdoc JSON），不用 syn 自研、不用 semver-checks | 自研 re-export 解析是个坑（xtask 的 syn 门禁只做局部模式匹配，量级不同）；semver-checks 面向发版语义，本仓 `publish = false`，要的是「变更可见」不是「semver 合规」 | syn 自研（维护负担）；semver-checks（目标错位）；不做（PR 里 API 漂移不可见） |
| D-26 | `AppConfig` 去 Deref 用 `engine()` / `engine_mut()` 访问器，不做字段平铺 | 访问器保住「引擎配置是一个整体注入物」的语义（D-06）；平铺 35 个字段制造同步负担 | 保留 Deref（反模式）；字段平铺（引入双份 truth） |
| D-27 | 测试合并采用 matklad one-binary 模式，但进程级 e2e 二进制豁免 | 链接单元数是冷测试的主要成本；但二进制间默认串行是 daemon/大文件类测试的既有隔离屏障，合并它们换来的收益补不回失去的稳定性 | 全量合并为每 crate 1 个（引入并发 flake）；不合并（33 个链接单元持续拖 CI） |
| D-28 | 空目录检查进 xtask layout 门禁 | 一次清理不防复发；layout 门禁已有 fs 遍历基础设施，边际成本几乎为零 | 只做一次性清理 |

---

## 4. 工作量与风险

| Phase | 预估 | 主要风险 | 缓解 |
|-------|------|----------|------|
| P1 | 半天 | 无 | — |
| P2 | 1 天 | de-Deref 的 81 个点位改漏（编译器兜底）；de-glob 后下游漏名字（编译器兜底） | 全程编译器驱动，零猜测 |
| P3 | 1 天 | clippy.toml 豁免覆盖不到 `tests/support` 辅助函数 | 文件头显式 allow，可 grep 审计 |
| P4 | **3–5 天（大头）** | 文档写成签名复述，通过门禁但无价值 | review 标准写进 PR 模板：抽查 20% 必须含「签名看不出的信息」；rules crate 与 DSL spec 交叉核对 |
| P5 | 1 天 | nightly rustdoc JSON 格式漂移导致 CI 偶发红 | pin 具体 nightly 版本，升级作为显式 PR |
| P6 | 1 天 | 合并后测试并发冲突（端口、env、cwd） | 合并前逐文件资源审计；e2e 类不合并（D-27） |

总计约 **8–10 个工作日**，6 个 PR，每个 PR 独立可回滚。P6 与其余 Phase 无依赖，可并行或放弃。

## 5. 完成定义（Definition of Done)

1. §1.1 的 workspace lints 全量生效，仓库零 `#[allow]` 新增（除成文豁免：xtask 的 missing_docs、测试辅助的 unwrap）；
2. `cargo doc --workspace --no-deps` 零警告，7 个库 crate 首页文档可读；
3. `cargo xtask check all` 含 api 快照与空目录检查，CI 全绿；
4. `docs/architecture.md` 新增 panic 政策与 API 快照纪律两节；
5. evidence 目录含 hardening-baseline 与 hardening-final 对照（测试二进制数、冷测试墙钟、doc 覆盖）；
6. 本文档移入 `docs/archive/`，状态改为已完成。
