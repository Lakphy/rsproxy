# rsproxy CLI 三轮 Dogfooding 报告

日期：2026-07-14  
起始代码基线：`c77bf08`（本地分支 `fix/shim-orphan-native-process-10`）<br>
最终源码：当前工作树（基于 `59c6b35`，包含完成审计的收口修复）<br>
环境：macOS / Darwin arm64，Rust 1.97.0  
产物：`target/release/rsproxy`（`rsproxy 0.0.1`）  
SHA-256：`8fa678866b1e787e68d3cc4b4cdf78138260ac93497495956127c77382a4619c`

## 结论

完成了三轮“编译 release → 真实操作 → 记录摩擦 → 当轮优化 → 回归验证”。CLI 的核心代理、规则、值、Trace、Replay、CA、系统代理预览和补全流程均可用；HTTP 与 HTTPS MITM 均经过真实网络请求验证。

最明显的改善是：默认输出从面向内部 API 的原始 JSON，变成面向人的摘要、表格和分节详情；`--json` 仍保留稳定机器接口。Replay 从仅支持 HTTP、可能无限等待，升级为支持 HTTP/HTTPS、服从统一超时、正确处理响应 framing 且只保留有界预览。

安全边界：本轮没有真正修改系统代理或系统信任库。`proxy on` 和 `ca install` 只执行了 `--dry-run`；CA 初始化、证书签发和导出只发生在 `target/dogfood-*` 隔离目录。

## 体验评分

以下是同一操作者在本轮开始和三轮优化结束后的主观评分，用于表达相对变化，不是基准测试：

| 维度 | 开始 | 结束 | 主要变化 |
| --- | ---: | ---: | --- |
| 上手顺滑度 | 7/10 | 9/10 | 根帮助与 quick start 已完整；常用查询不再要求先读 JSON |
| 命令合理性 | 8/10 | 9/10 | 命令族和 `--json` 约定一致，失败提示更贴近实际原因 |
| 输出易读性 | 5/10 | 9/10 | 状态、规则、值、Trace、Replay 均有 human-first 输出 |
| 功能完好度 | 7/10 | 9/10 | 补齐 HTTPS replay，并消除 replay 超时与 framing 缺陷 |
| 功能强度 | 8/10 | 9/10 | HTTP/HTTPS MITM、规则、Trace、Replay、CA、TUI、补全均实测 |
| 操作安全性 | 8/10 | 9/10 | 系统变更有 dry-run；错误建议区分服务端失败与连接失败 |

## 第一轮：首次上手、规则与 Trace

### 操作范围

- 从本地代码执行 `cargo build --release --locked -p rsproxy-cli`。
- 检查无参数、根帮助、版本、未启动时的 `status` 和恢复提示。
- 在隔离 storage 启动 daemon，执行 `status`、`rules check/set/test`。
- 通过代理访问本地 HTTP origin，确认规则生效，并执行 `trace ls/get`。

### 发现

- 代理、规则匹配、Trace 捕获功能正常，命令命名基本符合直觉。
- `status`、规则变更和 `trace get` 默认直接打印单行 JSON；人需要手工解析。
- `trace get` 会把较大的 body preview 整段刷到终端，难以快速定位状态、URL、规则和 headers。

### 当轮优化

- 新增统一 human-output 层：
  - `status` 按运行状态、端点、规则、Trace、MITM、DNS 分行摘要。
  - 规则变更返回动作、组名和当前有效规则数。
  - `trace get` 按基本信息、匹配规则、请求头、响应头和 body preview 分节。
  - 终端 body preview 限制为 2 KiB，并明确显示截断量。
- 所有相关命令的 `--json` 继续逐字返回控制 API JSON，避免破坏脚本。
- 生命周期测试显式使用 `--json` 解析状态，固定 human/machine 双输出契约。

### 验证结果

- daemon 状态变为可扫描的 `status=running ...` 摘要。
- 规则变更输出为 `updated rule group ...`。
- 真实 HTTP 请求经代理成功，Trace 详情分节展示；`--json` 保持原始 JSON。

## 第二轮：值、Mock、统计、TUI 与 Replay 可靠性

### 操作范围

- 使用第二个隔离 daemon 验证重复启动反馈。
- 执行 `values set/ls/cat`、Mock 规则、`rules test/ls`、真实 curl。
- 执行 `trace stats`、`tui --once`、`replay`、`trace clear`。
- 分别 replay 可访问本地 origin、Mock session 和不可访问 origin。

### 发现

- 值写入、Trace 统计和清理仍默认输出内部 JSON。
- replay Mock/不可达目标时，CLI 先在固定 5 秒后超时；daemon 实际仍健康，提示却像控制端点配置错误。
- replay 引擎使用无界 `TcpStream::connect` 和 `read_to_end`，没有复用 daemon 的 DNS、连接、TTFB 和总请求超时。
- 控制 API 错误以 `{"error":"..."}` 字符串包裹展示，增加阅读噪声。

### 当轮优化

- 为 `values set/rm`、`trace stats/clear`、`replay` 增加 human-first 输出，保留 `--json`。
- 控制客户端增加可配置请求超时 API，并更新公开 API 快照与契约测试。
- replay CLI 的控制等待时间改为 daemon 总请求超时再加 1 秒，不再抢先误判。
- replay 引擎复用 DNS resolver，并对 DNS、TCP connect、TTFB、response read 和总请求施加统一 deadline。
- 将控制超时、daemon HTTP 错误、端点不可达分成不同提示；human 与 JSON 错误均提取服务端真实 message。

### 验证结果

- 本地 HTTP replay 成功：`replayed id=1 status=200 bytes=6052`。
- 不可达目标约 0.21 秒返回真实 daemon 错误，不再等待固定 5 秒；随后 `status` 仍健康。
- TUI 单次快照、Mock、值管理、统计和清理均正常。

## 第三轮：CA、系统集成预览与 HTTPS

### 操作范围

- 执行 `ca status/init/issue/export` 和 `ca install --dry-run`。
- 执行只读 `proxy status` 与 `proxy on --service Wi-Fi --dry-run`。
- 生成 zsh completions。
- 以隔离 CA 启动 daemon，curl 通过 rsproxy 访问 `https://example.com/`。
- 获取 HTTPS Trace，并对捕获 session 执行 human/JSON replay。

### 发现

- CA 本地生命周期、系统操作预览和补全输出工作正常。
- HTTPS MITM 工作正常：CONNECT 成功，客户端与上游均协商 HTTP/2，Trace 记录状态 200、559 bytes 和 TLS/协议信息。
- CLI 将 replay 描述为 HTTP/HTTPS，但引擎拒绝 HTTPS，是明显的能力/文案不一致。
- 初次补齐 TLS 后，局部 TLS origin 测试进一步暴露 replay 依赖连接 EOF：
  - keep-alive + `Content-Length` 可能一直等待；
  - chunked body 没有解码；
  - `read_to_end` 可无界分配；
  - TLS 缺少 `close_notify` 会把已完整响应误判为失败。

### 当轮优化

- replay 支持 `http` 和 `https`，复用上游信任根与 TLS client config，并将 TLS handshake 纳入统一 timeout。
- 新增有界 HTTP/1 响应读取：
  - 正确处理 HEAD、1xx、204、304 无 body 响应；
  - 校验并按 `Content-Length` 精确读取；
  - 解码 chunked body，并限制 trailer 数量与大小；
  - 仅对 close-delimited 响应读到 EOF；
  - 精确统计响应字节，但内存中最多保留 64 KiB preview。
- 增加本地 HTTPS origin 和 chunked/大 preview 回归测试。
- 将 replay body reader、输出测试和 daemon 参数装配拆分到独立模块，使所有 Rust 文件满足 500 行架构上限，并同步 Whistle 进程配置证据扫描。

### 最终 release 冒烟

最终产物被重新构建并用新进程启动，未复用第三轮旧 daemon：

```text
status=running version=0.0.1
https_status=200 protocol=2
1  http  200  ...  559  GET  https://example.com/
replayed id=1 status=200 bytes=559
```

JSON replay 返回完整结构化对象；不存在的 session 返回单个稳定错误文档，message 为 `not found`，没有二次 JSON 包裹。冒烟结束后 daemon 已停止。

## 三轮后的完成审计与收口

三轮结束后，又以当前工作树为权威重新构建 release，并从干净的隔离 storage 复跑启动、状态、值、规则、真实 HTTP 转发、Mock、Trace、Replay、TUI、统计、清理和停止。该审计不计作新的 dogfooding 轮次，用于验证报告没有依赖旧二进制或旧 daemon。

审计发现并直接修复了三个收口问题：

- `trace` 帮助仍示例 `trace get 42 | jq`，但三轮优化后默认输出已是 human format；现改为先展示 human 用法，再明确示例 `trace get 42 --json | jq`，并增加帮助回归测试。
- 根帮助 quick start 用空格对齐两个连续命令，实际渲染后层次不清；现把 dry-run 预览和真实变更拆成独立编号步骤。
- 规则解析器会接受 `res.header("x-debug: yes")` 这类带引号的非法 header 名，并把引号写到真实 HTTP 响应；现对 set/remove/replace/trailer 的名字统一执行 HTTP token 校验，错误直接提示使用 `res.header(x-debug: yes)` 这样的无引号名称。

最终 release 的真实链路证据：

```text
invalid rule exit=1
error: ... invalid header name `"x-debug`; use an unquoted HTTP header name such as `x-debug`

updated rule group default: 1 rule(s) active
HTTP/1.0 200 OK
X-Rsproxy: dogfood
1  http  200  ...  GET  http://127.0.0.1:18082/
```

第一次收口冒烟在启动本地 Python origin 后立即请求，因 origin 尚未 ready 得到一次 502；增加显式 readiness 检查后使用相同最终 release 重跑得到以上 200 结果。该失败属于测试编排竞态，不是 rsproxy 代理回归。

## 三轮解决的问题汇总

1. 常用命令默认输出 raw JSON，人工阅读成本高。
2. Trace 详情无层次且 body preview 容易刷屏。
3. 值变更、Trace 统计/清理和 Replay 的输出风格不一致。
4. Replay 控制请求固定 5 秒超时，可能早于 daemon 的实际操作超时。
5. Replay 没有 DNS/connect/TTFB/read/total timeout，可能长时间挂起。
6. Replay 失败提示错误地引导用户检查控制端点。
7. 服务端错误被 JSON transport wrapper 包裹。
8. 文案声称支持 HTTPS Replay，但引擎只支持 HTTP。
9. Replay 错误依赖 EOF，无法稳健处理 keep-alive、Content-Length 和 chunked 响应。
10. Replay 响应读取存在无界内存增长风险。
11. TLS 完整响应可能因缺少 `close_notify` 被误报为失败。
12. 新增实现和当前分支已有文件触发 500 行/测试布局架构门禁；已结构化拆分并恢复全绿。
13. `trace get` 的帮助示例忘记在接 `jq` 前启用 `--json`。
14. 根帮助 quick start 的 dry-run 与真实变更步骤对齐不清。
15. header 动作接受非法 header 名，可能把带引号的畸形字段发到网络上。

## 剩余优化计划

### P1：跨重启读取持久化 Trace

现状：spill segment 会保留在磁盘并计入 `trace stats`，但 daemon 重启后 `trace ls/get/replay` 只查询当次进程内存，无法直接操作历史 spill session。

计划：为 list/detail/replay 增加内存 + spill 的统一只读视图，定义 ID 去重、排序、损坏 segment 跳过和并发 clear 的一致性语义；补充“捕获 → 停止 → 启动 → list/get/replay”集成测试。

### P1：Replay 请求体完整性

现状：Trace body 是有界 preview。大请求 replay 时可能只发送捕获前缀，输出没有明确说明是否截断。

计划：在 Session/ReplayResponse 中暴露 `request_body_complete`；默认拒绝不完整请求 replay，提供显式 override，并在 human/JSON 输出中显示风险。

### P2：Replay 路由语义显式化

现状：Replay 按设计直连原始 origin，不经过规则和 upstream route，但从命令输出不容易看出。

计划：输出 `route=direct-origin`，并评估 `--through-proxy` 或 `--apply-rules` 模式；在帮助中写明是否产生新 Trace。

### P2：输出契约扩展

计划：为 human 输出增加端到端 golden snapshots，并在检测到 TTY 时谨慎加入颜色；非 TTY 和 `--json` 保持无颜色、稳定、可管道处理。

## 验证清单

```text
cargo fmt --all -- --check
cargo xtask check all
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --all-targets --locked
cargo build --release --locked -p rsproxy-cli
```

结果：全部通过。Workspace 测试包含 CLI 生命周期与产品矩阵、Control、Engine（207 项）、Network、Platform、Rules（含 Whistle 兼容矩阵）、Trace、公开 API、bench target 和 xtask；仓库显式忽略的 1 GiB release 资源压测未执行。
