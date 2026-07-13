# rsproxy 技术方案

> Rust 实现的高性能调试代理（HTTP / HTTPS / HTTP2 / WebSocket），对标 whistle 的核心能力：规则路由引擎 + 请求 Trace，性能优先，纯 CLI 交互。
>
> 版本：v0.2 · 2026-07-13

---

## 目录

1. [背景与目标](#1-背景与目标)
2. [whistle 源码研究结论](#2-whistle-源码研究结论)
3. [总体架构](#3-总体架构)
4. [技术选型](#4-技术选型)
5. [代理内核设计](#5-代理内核设计)
6. [规则路由引擎设计](#6-规则路由引擎设计)
7. [请求 Trace 设计](#7-请求-trace-设计)
8. [控制平面与 CLI 设计](#8-控制平面与-cli-设计)
9. [性能设计](#9-性能设计)
10. [测试策略](#10-测试策略)
11. [项目结构](#11-项目结构)
12. [里程碑规划](#12-里程碑规划)
13. [结构改革后的遗留项](#13-结构改革后的遗留项)

---

## 1. 背景与目标

### 1.1 背景

whistle 是 Node.js 实现的跨平台调试代理，核心价值在于：

- **一套文本规则 DSL**（`pattern operation filters...`）统一表达 hosts、转发、mock、请求/响应改写、限速等全部调试能力；
- **全量请求抓包**（Network 面板）辅助排查问题。

但 whistle 受限于 Node.js 单线程模型与动态语言开销，在高并发、大 body 场景下 CPU / 内存占用偏高，且插件体系带来了大量动态加载复杂度。

### 1.2 目标

rsproxy 用 Rust 重写这套能力，目标排序（冲突时按此优先级取舍）：

1. **性能高于一切**：转发路径零拷贝、无锁热路径、规则匹配微秒级，代理开销接近透明转发；
2. **规则引擎能力对齐 whistle、语法全新设计**：能力集覆盖 whistle 核心（匹配：域名/路径/精确/通配符/正则/端口/排除；动作：转发/mock/改写/流控；条件：方法/头/体/IP/状态码/概率；值引用与模板变量），但 DSL 语法不继承 whistle 历史包袱，按一致性原则重新设计（见 [§6.1](#61-语法全新-dsl)），全部有完整测试覆盖；
3. **Trace 详细且资源可控**：记录完整请求生命周期（时序、头、体、命中规则、连接信息、WS 帧），通过内存预算 + 环形缓冲 + 磁盘落盘 + 采样策略保证低占用；
4. **全功能 CLI**：daemon + 控制客户端模式，规则管理、trace 查看/跟踪/导出、证书管理、系统代理设置全部可在 CLI 完成，另提供 TUI 实时视图。

v1 的发布与运行验收环境固定为当前开发机：Apple M1 Pro（8 核，其中 6 个性能核）
上的 macOS ARM64。Linux/Windows 兼容代码和 workflow 可以继续保留，但不要求目标 OS
运行、hosted runner 或多平台产物证据，也不计入 v1 完成度。

### 1.3 非目标（明确不做）

| 不做的能力 | 原因 |
| --- | --- |
| 插件体系（`plugin://`、pfork 子进程、插件 UI） | Rust 无动态加载友好性，v1 移除；后续单独设计新动态化方案（候选：WASM 组件模型 / 外部进程 gRPC 扩展点，见 [§13](#13-风险与开放问题)） |
| Web UI | v1 仅 CLI；但控制平面 API 按「未来可直接挂 Web UI」设计 |
| 脚本类规则（`reqScript` / `resScript` / `rulesFile` 动态脚本） | 依赖 JS 运行时，属于动态化能力，随插件体系一并延后 |
| weinre / composer(重放构造器高级功能) / SAZ 导入导出 | 非核心，HAR 导出替代 |
| 集群模式（`--cluster`） | Rust 多线程 runtime 单进程即可吃满多核 |
| HTTP/3 / QUIC | 系统代理场景下客户端走不了 QUIC 会自动回落 h2/h1（行业现状同 Charles/whistle/mitmproxy）；透传模式不放行 UDP，故不存在 h3 绕过抓包的问题 |
| WebSocket over h2（RFC 8441）、h2c（明文 h2 upgrade） | 现实流量占比极低，v2+ 视需求 |
| SOCKS5 UDP associate | 不做（UDP 转发超出调试代理定位）；SOCKS5 TCP 接入为 v2 |
| Linux/Windows 目标 OS 运行与多平台发布资格 | 当前产品只保障本机 macOS ARM64 效果；其他平台实现按 best-effort 保留，不作为 v1 验收项 |

> 完整的请求形式支持状态见 [附录 B：网络请求形式覆盖矩阵](#附录-b网络请求形式覆盖矩阵)。

### 1.4 已对齐的关键决策

| 决策点 | 结论 |
| --- | --- |
| 规则 DSL 语法 | **全新设计**，不兼容 whistle 语法；仅对齐能力模型与优先级语义。v2 提供 whistle 规则静态迁移转换器 |
| 正则引擎 | regex crate 为主（线性时间）；含 backreference / lookaround 的规则**自动降级 fancy-regex**，强制执行预算防回溯 DoS |
| HTTPS MITM 默认策略 | **默认全量解密**（Charles / mitmproxy 模式，与 whistle 默认透传相反）；`bypass` 规则 / `--no-mitm` 排除 |
| 控制平面传输 | **unix socket 优先**（0600 免鉴权），`--api` 可选开 127.0.0.1 TCP（强制 token）；Windows 用命名管道适配 |
| v1 验收平台 | **当前 Apple M1 Pro / macOS ARM64**；跨平台编译与 workflow 不阻塞发布 |

---

## 2. whistle 源码研究结论

> 研究对象：Whistle v2.10.5。完整上游 checkout 已在研究完成后移除；合同测试只保留
> `crates/rsproxy-rules/tests/fixtures/whistle-2.10.5/` 最小证据快照。

### 2.1 整体结构

```
whistle/lib
├── index.js          # 入口：express app + 多端口 server（主端口/http/https/socks）
├── config.js         # 配置（1194 行，CLI 参数 + .whistlerc + 环境变量合并）
├── tunnel.js         # CONNECT 隧道处理（884 行）：解密 or 透传决策
├── upgrade.js        # WebSocket upgrade 处理
├── socket-mgr.js     # 连接管理（958 行）：每域名 256 连接池
├── rules/
│   ├── rules.js      # 规则引擎核心（2475 行）：解析 + 匹配 + 值解析
│   ├── protocols.js  # 74 个协议注册表 + 别名表 + req/res 分类
│   ├── dns.js        # DNS 解析 + 缓存（默认 60s）
│   ├── storage.js    # 规则/值的多分组持久化存储
│   └── util.js       # 规则/值的运行时管理（分组启停、合并）
├── https/
│   ├── ca.js         # node-forge 实现的 CA：根证书 + 按域名动态签发 + LRU 缓存
│   ├── index.js      # HTTPS MITM 接入、SNI 分发
│   └── h2.js         # HTTP/2 支持
├── inspectors/       # 中间件式抓包：rules → req → data → res
│   ├── rules.js      # 为每个请求解析规则快照
│   ├── req.js        # 请求侧改写（头/体/延迟/限速）
│   ├── res.js        # 响应侧改写（1332 行）
│   └── data.js       # trace 采集：body 上限 360KB，会话缓存默认 600 条
├── handlers/         # 最终处理：http-proxy（上游转发）、file-proxy（本地 mock）
└── service/          # UI 数据服务 data-center：定时快照 + 拉取模型
```

### 2.2 请求处理管道

whistle 用 express 中间件串联整个生命周期（`lib/index.js:40`）：

```
request → init(标准化, reqId, 客户端信息)
        → biz(内置 UI/API 短路)
        → inspectors/rules(规则匹配快照挂到 req)
        → inspectors/req(请求改写 + 请求 trace)
        → inspectors/data(trace 采集编排)
        → inspectors/res(响应改写 + 响应 trace)
        → handlers/http-proxy | file-proxy(实际转发或本地响应)
```

要点：**规则匹配一次、结果挂载、后续阶段各取所需**。请求阶段与响应阶段使用不同协议子集（`protocols.js` 中 `reqProtocols` / `pureResProtocols`），响应阶段还会基于响应属性（状态码、响应头）二次评估 filter。rsproxy 沿用该「一次匹配 + 两阶段应用 + 响应期再过滤」模型。

### 2.3 规则引擎

规则文本模型：**每行 = `pattern operation... [filters...]`**，行内空白分隔，`#` 注释。

**pattern 的 7 种形态**（`rules.js: parseRule/parseWildcard/isRegUrl`）：

| 形态 | 示例 | 语义 |
| --- | --- | --- |
| 域名 | `example.com`、`example.com:8080`、`.example.com` | 域名（可带端口）匹配任意协议；`.` 前缀含子域名 |
| 路径前缀 | `example.com/path/to`、`https://a.com/p` | 路径按 `/` 边界前缀匹配；带 `?` 则路径精确 + query 前缀 |
| 精确 | `$https://a.com/p`、`$a.com/p?q=1` | `$` 前缀，路径精确（带 query 则 query 也精确） |
| 通配符 | `^https://*.a.com/p/**`、`*.a.com` | 域名位 `*`（不跨 `.`）/`**`（跨 `.`）；`^` 开启路径/query 通配 `*`/`**`/`***`，尾部 `$` 收尾 |
| 正则 | `/user\/(\d+)/i` | JS 正则语义，`i` 忽略大小写 |
| 端口 | `:8080`、`!:9090` | 仅按端口匹配 |
| 排除 | `!example.com/x` | `!` 前缀反选 |

通配符与正则的捕获组可在 operation 中以 `$0`-`$9` 引用（子匹配传值）。whistle 内部把通配符**编译为正则**并带 `regUrlCache` 缓存（`rules.js:66`）。

**operation（协议）**：74 个内置协议 + 别名表（`protocols.js`）。按用途分类：路由转发（host/proxy/socks…）、本地 mock（file/rawfile/tpl/statusCode/redirect…）、请求改写（reqHeaders/reqBody/reqReplace/method/ua/auth…）、响应改写（resHeaders/resBody/resReplace/resCors/attachment…）、流控（reqDelay/resDelay/reqSpeed/resSpeed）、控制（ignore/filter/enable/disable/delete/lineProps）。部分协议**可多条同时生效**（`multiMatchs`：reqHeaders、resHeaders、各类 replace/append/prepend 等），其余协议**首条命中即终止**。

**filter**：`includeFilter://` / `excludeFilter://`，支持 `m:`（方法）、`b:`（请求体）、`i:`/`clientIp:`/`serverIp:`（IP）、`s:`（状态码，响应期评估）、`reqH.key:`/`resH.key:`（头）、`chance:`（概率）以及嵌套 URL pattern。多 filter 之间为「或」，include 与 exclude 组合为「且」。

**值系统**：`{key}`（引用 values 存储）、`(inline)`（行内值）、`<path>`（文件路径值）、模板变量 `${url}` `${method}` `${now}` `${reqH.xx}` 等 40+ 个（`TPL_VAR_RE`，`rules.js:78`）。

**优先级模型**：规则文本自上而下，同协议首条命中生效（multiMatch 除外）；`lineProps://important` 可插队；多分组（Default + 命名分组）按分组启用顺序合并。

### 2.4 HTTPS MITM 与隧道

- CA：node-forge 生成根证书，按域名动态签发叶子证书 + LRU 缓存（`https/ca.js:262`）；SNI 回调按 servername 取证书。
- CONNECT 隧道（`tunnel.js`）：按规则决定「解密拦截」（进入 HTTP 管道）或「透传」（直接对拷字节流）；透传仍产生 tunnel 类型的 trace 记录。
- HTTP/2：按 ALPN 协商，h2 ↔ h1 双向桥接。

### 2.5 Trace（抓包）机制与资源控制

- 采集点内嵌在 inspectors 管道中，请求/响应 body 采集有硬上限：普通 body 360KB、req/res 流式上限 2MB、strict 模式收紧到 256KB（`inspectors/data.js:7-11`）。
- 会话缓存为**有限队列**：默认 600 条（`-R reqCacheSize`），WS 帧缓存 512 条（`-F frameCacheSize`），超出丢弃最旧。
- UI 采用**拉取模型**（data-center 定时快照 + 客户端轮询 + 增量 id 游标），采集本身不阻塞代理路径。
- 记录字段：完整时序（dns/connect/ssl/ttfb/end）、双向头、body（截断标记）、命中规则快照、客户端/服务端地址、协议版本、WS/SSE 帧。

### 2.6 CLI

`w2 start|stop|restart|run|status|add|proxy|ca|use/exec...`，daemon 化靠 starting/pfork 库；`proxy` 子命令直接改系统代理，`ca` 管理根证书安装。**结论**：whistle CLI 只覆盖启停与安装配置，规则/抓包必须走 Web UI —— **rsproxy 的 CLI 必须补齐这块**（规则 CRUD、trace 查询/跟踪/导出），这是与 whistle 的最大交互差异。

### 2.7 对 rsproxy 的设计启示

| whistle 做法 | rsproxy 取舍 |
| --- | --- |
| 一次匹配挂载 + 两阶段应用 | 沿用，规则快照为不可变 `Arc<MatchedRules>` |
| header 上限吃 Node 默认 16KB（`HPE_HEADER_OVERFLOW`→431，只能 NODE_OPTIONS 全局调） | 默认放宽到 256KB/256 条、双侧生效、`--max-header-size` 可调（见 §5.1） |
| 通配符统一编译成正则 | 沿用编译思路，但增加**域名索引前置过滤**，避免万级规则线性扫描（whistle 是 per-protocol 线性数组扫描） |
| body 采集上限 + 会话有限队列 | 沿用并扩展：全局内存预算 + 磁盘落盘 + 采样 |
| express 动态中间件 | 改为静态编译的处理管道（enum dispatch），无动态分发开销 |
| 拉取式 UI 数据模型 | 控制平面同时支持拉取（分页查询）与推送（follow 流式） |
| 插件/脚本扩展 | v1 移除，预留扩展点 trait |
| 规则 DSL 语法 | 仅继承能力模型与「文本顺序 + 分组 + important」优先级语义；语法全新设计（通配符行为处处一致、条件显式化、动作命名空间化），高频规则可用迁移转换器（v2）静态转换 |

---

## 3. 总体架构

```
                                   ┌────────────────────────────────────────────┐
                                   │                rsproxy daemon               │
                                   │                                            │
 client ──HTTP/CONNECT/WS──▶ ┌─────┴─────┐    ┌──────────────┐    ┌───────────┐ │
                             │ Listener  │───▶│  Proxy Core   │───▶│ Upstream  │─┼──▶ origin
                             │ (多端口)   │    │  (处理管道)    │    │ Connector │ │
                             └─────┬─────┘    └──┬───────┬───┘    └───────────┘ │
                                   │             │       │                      │
                                   │      ┌──────▼──┐ ┌──▼────────┐             │
                                   │      │ RuleSet │ │  Tracer   │             │
                                   │      │(ArcSwap)│ │(mpsc→存储) │             │
                                   │      └──────▲──┘ └──▼────────┘             │
                                   │             │   ┌────────────┐             │
                                   │      ┌──────┴─┐ │ TraceStore │             │
                                   │      │ Rules  │ │ 内存环形+磁盘│             │
                                   │      │ Store  │ └──▲─────────┘             │
                                   │      └──────▲─┘    │                       │
                                   │   ┌─────────┴──────┴────────┐              │
                                   │   │  Control Plane (API)    │              │
                                   │   │  unix socket + 127.0.0.1│              │
                                   │   └─────────▲───────────────┘              │
                                   └─────────────┼──────────────────────────────┘
                                                 │ JSON API
                              ┌──────────────────┼──────────────────┐
                              │ rsproxy CLI 子命令 │  rsproxy tui     │ (未来 Web UI)
                              └──────────────────┴──────────────────┘
```

四个平面：

1. **数据平面（Proxy Core）**：监听 → 协议识别（HTTP/CONNECT/WS/TLS）→ 规则匹配 → 改写/mock/转发 → 响应改写。全异步、零拷贝流式。
2. **规则平面（Rule Engine）**：文本 DSL 解析 → 编译为不可变 `CompiledRuleSet` → `ArcSwap` 原子热更新。独立 crate，可脱离代理单测。
3. **观测平面（Trace）**：数据平面通过有界 mpsc 发事件（永不阻塞热路径），collector 聚合为 Session 写入内存环形缓冲，可选落盘。
4. **控制平面（Control Plane + CLI）**：daemon 暴露 JSON API（unix socket 优先 + 127.0.0.1 TCP 可选），CLI 与 TUI 均为该 API 的客户端；未来 Web UI 直接复用。

---

## 4. 技术选型

| 领域 | 选型 | 理由 / 替代方案对比 |
| --- | --- | --- |
| 异步运行时 | **tokio**（multi-thread） | 生态最全（hyper/rustls/tungstenite 全兼容）。monoio/glommio 的 io_uring 收益在「代理 + 规则 + trace」混合负载下不显著，且牺牲生态与 macOS 支持；预留 runtime 抽象最小化，不为其过度设计 |
| HTTP | **hyper 1.x**（+ hyper-util） | 低层 API 可精细控制 h1/h2、upgrade、流式 body；不用 axum/actix 做数据平面（框架抽象带来拷贝与开销）。控制平面 API 用 **axum**（复用同一 hyper） |
| TLS | **rustls 0.23** + tokio-rustls | 纯 Rust、无 openssl 构建负担、性能优于 openssl 绑定；`ServerConfig::cert_resolver` 支持按 SNI 动态出证书 |
| 证书签发 | **rcgen** | 生成根 CA 与按域名叶子证书；替代 node-forge 角色 |
| HTTP/2 | hyper 内置 h2 | ALPN 协商，h1↔h2 桥接 |
| WebSocket | **tokio-tungstenite**（拦截模式）/ 原生双向对拷（透传模式） | 仅在需要帧级 trace 或帧改写时解析帧，否则纯字节转发 |
| DNS | **hickory-resolver** | 异步、内置 TTL 缓存与负缓存，支持自定义 DNS server（对齐 `--dnsServer`） |
| 规则匹配 | **regex** + **fancy-regex** 兜底 + **aho-corasick** + 自研域名索引 | regex crate 保证线性时间（无回溯灾难，天然抗 ReDoS）；解析期检测到 backreference/lookaround 自动改用 fancy-regex（带执行预算：步数上限 + 超时熔断，超限按不命中处理并计数告警）；aho-corasick 做多模式预过滤 |
| 规则解析 | 手写递归下降解析器 | 行式 DSL 语法简单，手写比 nom/pest 更快、错误信息更可控 |
| 热更新 | **arc-swap** | 规则集无锁读，更新原子替换 |
| 缓存 | **moka**（证书/DNS/正则编译缓存） | 并发 LRU/TTL |
| 字节缓冲 | **bytes** | 引用计数零拷贝切片，body 流转不复制 |
| CLI | **clap**（derive） | 子命令 + 补全生成 |
| TUI | **ratatui** + crossterm | 实时会话表 + 详情面板 |
| 序列化 | serde / serde_json | 控制 API 与 trace 导出 |
| 内部日志 | **tracing** + tracing-subscriber | 注意与「请求 Trace」区分：tracing 是进程日志 |
| 磁盘 trace 存储 | 自研 append-only 分段文件（+ **zstd** 可选压缩） | 见 [§7.4](#74-磁盘落盘spill)；不用 sqlite（写放大、锁）；不用 redb（查询模式简单，不值引入） |
| daemon 化 | 自研（fork + pidfile + unix socket 健康检查） | Rust 无 pm2 等价物，逻辑简单自建 |
| 系统代理 | 自研平台适配（macOS `networksetup` / Windows 注册表 / Linux gsettings+env 提示） | 对齐 `w2 proxy` |
| 测试 | cargo test + **insta**（快照）+ **proptest** + **cargo-fuzz** + **criterion**（bench）+ **wiremock/自建 hyper 桩**（集成） | 见 [§10](#10-测试策略) |

当前进程日志已由 `logging.rs` 统一接入 `tracing-subscriber`，支持
`RSPROXY_LOG` > `RUST_LOG` > workspace crate targets `=info` 的 filter 优先级和
`RSPROXY_LOG_FORMAT=text|json`。日志固定写 stderr，启动、监听、trust root、
连接错误和 session 完成/失败使用稳定事件字段；它与 §7 的请求 Trace 没有存储或
生命周期耦合。真实二进制黑盒测试会解析 JSON 日志并验证端口 0 的实际监听地址。

MSRV：Rust 1.88；edition 2024。单二进制发布（musl 静态链接可选）。

---

## 5. 代理内核设计

### 5.1 监听与协议识别

- 主端口默认 `8899`（与 whistle 一致，减少迁移成本），同端口同时接受：
  - 普通 HTTP 代理请求（绝对 URI）
  - `CONNECT` 隧道（HTTPS/任意 TCP）
  - WebSocket upgrade
  - 直连模式请求（作为反代/被 iptables 转发时的 Host 路由）
- 可选附加端口：`--http-port`（纯 HTTP）、`--https-port`（TLS 接入 + SNI 路由）、`--socks-port`（SOCKS5，v2 里程碑）。
- 每 accept 一个连接 spawn 一个 task；连接内按 keep-alive 复用处理循环。
- **代理接入认证**：`--proxy-auth user:pass` 开启 Proxy-Authorization（Basic）校验，未认证返回 407；`0.0.0.0` 监听（局域网/移动端调试）场景防裸奔。CONNECT 与普通代理请求同样生效。Basic scheme 按 HTTP 语义大小写不敏感并容忍 OWS，启动期拒绝缺少分隔符或空用户名/密码的配置；认证成功后立即从请求对象移除 `Proxy-Authorization`，不进入规则、上游请求、内存 trace、磁盘 spill 或导出文件。
- **大 header 默认适配**：whistle 受 Node 默认 16KB header 上限约束（超限 `HPE_HEADER_OVERFLOW` → 431，需手动 `NODE_OPTIONS` 调大），是已知痛点。rsproxy 默认 **单请求 header 总量 256KB、条数 256**，四个方向（客户端接入 h1 缓冲 / h2 `SETTINGS_MAX_HEADER_LIST_SIZE`、上游 h1/h2）同值生效，`--max-header-size` 与 `--max-header-count` 可调；该缓冲按需增长非预分配，放宽默认值无常驻内存代价。超限仍返回 431，但错误体明确说明超了哪个限制、如何调整。

当前可执行子集：普通代理和 CONNECT MITM 内层 HTTP/1.x 均在同一客户端连接上循环处理请求，HTTP/1.1 默认持久、`Connection` / `Proxy-Connection: close` 终止，HTTP/1.0 默认关闭且仅在显式 `keep-alive` 时复用。解析器先完成 request head 与 framing 校验，再由状态化 reader 按 Content-Length 或 chunked/trailer 边界消费 body；reader 支持有界聚合后从同一字节位置无损续传，因此已到达 socket 的 pipeline 请求仍会按序处理和响应。每个复用请求独立进入规则、上游和 trace 管道。响应统一重建 Content-Length 或 chunked 终止边界并输出唯一 Connection 头；HTTP/1.0 响应固定使用 1.0 状态行、Content-Length 且抑制 trailers。WebSocket upgrade、真正流式 SSE 和 CONNECT 接管 socket 后退出 HTTP 循环。空闲/慢读复用连接的 socket 超时为 90 秒；trace flags 标记 `h1-client-keepalive` / `h1-client-close` / `h1-client-connection-reused`，MITM 复用另标记 `mitm-tunnel-reused`，复用 TLS 上下文保留协议/套件但握手耗时记 0。成功 CONNECT 响应不再发送与后续隧道生命周期矛盾的 `Connection: close`。

### 5.2 请求处理管道（静态编译，替代 express 中间件）

```rust
// 伪代码：阶段静态串联，无动态分发
async fn handle(req) {
    let ctx = RequestCtx::new(req);              // reqId、时间戳、客户端信息
    let rules = ruleset.load().resolve(&ctx);    // ① 一次匹配 → Arc<MatchedRules>
    tracer.emit(SessionStart(&ctx, &rules));     // ② trace: 非阻塞 try_send

    if rules.skip_all() { return forward_direct(ctx).await }

    apply_req_rewrites(&mut ctx, &rules).await;  // ③ 请求改写(头/体/方法/延迟/限速)
    let resp = dispatch(&ctx, &rules).await;     // ④ mock(file/statusCode/redirect)
                                                 //    或上游转发(host/proxy/直连)
    let resp = apply_res_rewrites(resp, &rules,  // ⑤ 响应改写(含响应期 filter 复评)
                                  &mut ctx).await;
    tracer.emit(SessionEnd(...));                // ⑥ trace 收尾
    resp
}
```

关键点：

- **规则快照**：`resolve()` 产出 `MatchedRules { first: EnumMap<ActionKind, Option<Arc<Rule>>>, stacked: ... }`，请求生命周期内不可变；响应期需要状态码/响应头的 filter 在 ⑤ 阶段惰性复评（对齐 whistle 的 res 阶段 `resolveRules`）。
- **body 处理分级**：无 body 相关规则且 trace 不采集 body → 纯流式 `Bytes` 透传；仅 trace 采集 → tee 旁路（复制引用不复制数据，达到上限即停止）；有 body 改写规则 → 有上限聚合（默认 8MB，超限拒绝改写并标记）后应用改写。
- **错误处理**：上游错误转换为 502/504 响应并写入 trace（whistle 的 error-handler 等价物）。

当前请求方向可执行子集：普通和 MITM 下游 HTTP/1 先在 request head 后完成代理认证，再按候选规则判断是否需要 body；`Expect: 100-continue` 由 rsproxy 本地响应且不会转发给 origin。客户端 h2 的 DATA/trailers 由容量固定的通道送入同一 request-body reader 和规则计划，不再先收集完整 `Incoming`。小 body 在 `body_buffer_limit` 内聚合并保留现有 h1/h2 上游池和完整 body 规则语义；h1/h2 超限 body 都从同一 reader 无损续传：可协商 h2 且不是 WebSocket/SSE/请求限速时，通过容量 8 的 DATA/trailer/error 通道发送到 cold 或 pool-hit origin h2 stream；origin ALPN 回落 h1 或不适合 h2 时，继续使用独占手写 origin h1，Content-Length 或 chunked data 重新编码并保留 trailers。超限时 body-independent matcher/action 继续生效，body condition 与 mutation 跳过并标记 `request-body-rewrite-skipped-limit`；流式 trace 在上传期间发送有界 `BodyChunk`，最终 `BodySnapshot` 校正准确总字节数和前缀。下游读取、h2 channel 背压和上游写入共享 request-total 剩余 deadline；流式 h2 的 TTFB 预算从 request body 关闭后开始，提前到达的响应头直接复用；短路/上游提前结束且未消费完整 body 时关闭 h1 客户端连接或取消对应 h2 请求流。Loop 95 在 1MB 聚合上限下经 curl 背压完整上传 64MB，daemon RSS 从 11,120KB 稳定到 11,728KB；另验证了 chunked trailer、规则降级和 HTTPS MITM 2MB 上传。本轮自动化使用 h1 与 h2 客户端分别向只宣告 h2 的 TLS origin 上传 1.125MB，origin 均在客户端发送剩余 1MB 前收到 DATA，最终字节与 request trailers 保真；另覆盖同一 h2 session 的 pool-hit。该新路径尚未增加 Dogfooding 轮次，超限请求仍不进入共享 h1 pool。

当前响应方向可执行子集：Hyper h1/h2 在收到上游响应头后即返回一个容量固定的 `Bytes` frame 通道，普通 HTTP/1.1 下游以 chunked framing 边读边写；流式 trace 在响应期间发送有界 `BodyChunk` 且仅保留配置的 body 前缀，trailers 在流结束时保真输出。返回客户端 h2 时，bridge 同样在统一管道写出响应头后立即发布 h2 head，再把 Content-Length、chunked/trailers 或 close-delimited body 增量解码到容量固定的 DATA/trailer 通道，不再 capture 完整响应；HEAD、1xx、204、304 不发 DATA。h1 独占连接租约和 h2 stream 租约均持续到 body/trailers 完整结束；响应头之后的上游 body 错误只关闭当前下游响应并写入 session error，不再追加第二个 502。只有命中 `res.body`、`res.merge`、`inject`、`delete(resBody)` 或 `delete(resBody.*)` 时才聚合，默认上限 8MB，可用 `--body-buffer-limit` / `body_buffer_limit` 调整；超限时完整原样流式返回，跳过 body 改写并标记 `body-rewrite-skipped-limit`。Loop 94 使用 8MB/s 背压 curl 完整传输 64MB，进程 RSS 连续采样稳定在约 23.5MB，trace 仅保留 4KB 前缀。本轮新增真实 TLS+h2 自动化测试，证明客户端在 origin 完成 2MB 分块响应前已收到 head 和首个 DATA，并保留 origin 与规则 response trailers；该新路径尚未增加 Dogfooding 轮次。

### 5.3 CONNECT 隧道与 HTTPS MITM

决策树（**默认全量解密**，对齐 Charles/mitmproxy 的开箱体验；与 whistle 的默认透传相反）：

```
CONNECT host:port
  ├─ 规则命中 bypass 或 --no-mitm 全局关闭 ─▶ 透传（纯双向对拷，仍记 tunnel trace）
  ├─ 默认 ────────────────────────────────▶ TLS MITM：
  │       rcgen 按 SNI 签发叶子证书(moka 缓存, 容量默认 1024)
  │       → ALPN 协商 h1/h2 → 回到 §5.2 HTTP 管道
  │       客户端侧握手失败(未信任 CA / 证书固定 app)
  │       → 记录错误并对该 host 自动降级透传（带 TTL 记忆，
  │         --strict-mitm 可关闭降级以暴露问题）
  │       上游侧要求客户端证书(mTLS)且未配置
  │       → 当前请求返回明确错误；已配置 tls(client-cert=…, client-key=…)
  │         的直连 HTTPS origin 规则正常完成双向认证
  ├─ 非 TLS 流量(探测首字节) ──────────────▶ 按 HTTP 管道处理（web 型隧道）
  └─ 其余 ───────────────────────────────▶ 透传
```

- 根 CA：首次启动生成（ECDSA P-256，10 年），存 `~/.rsproxy/ca/`；`rsproxy ca install` 安装到系统信任库，`rsproxy ca export` 导出（含二维码文本形式方便移动端，v2）。
- 叶子证书：SNI → 通配符归一（`a.b.example.com` → `*.b.example.com`）→ 缓存命中或 rcgen 现签（ECDSA，签发耗时 <1ms）。
- 透传模式使用 `tokio::io::copy_bidirectional`，仅统计字节数与时序进 trace。

当前 HTTPS MITM 可执行子集：CONNECT MITM 已支持客户端侧与上游侧 TLS 握手观测；rustls server config 按优先级宣告 `h2` / `http/1.1`，客户端选择 h2 时进入 Hyper stream bridge，选择 h1 时保留原管道；origin TLS client config 同样按优先级宣告 `h2` / `http/1.1` 并按实际 ALPN 分派上游协议，HTTPS proxy hop 自身仍只宣告 `http/1.1`。上游验证根由内置 WebPKI、`rustls-native-certs` 读取的 macOS/Windows/Linux 原生 trust store（含 `SSL_CERT_FILE` / `SSL_CERT_DIR` 覆盖）和组合根启动时通过 platform 读取并注入 `ProxyConfig` 的 rsproxy CA 合并；WebPKI/原生 anchor 与注入 CA 均从内存材料加载、去重并按进程缓存，engine 不发现或读取 root CA 文件，`status.upstream_roots` 暴露加载/拒绝/重复/错误/总数。MITM 叶子证书仍落盘到 `<storage>/ca/leaf`，运行期另有 `ServerConfig` LRU 内存缓存，默认容量 1024，可用 `--mitm-cert-cache-capacity` 调整或设为 0 关闭，trace flags 标记 `mitm-cert-cache-hit` / `mitm-cert-cache-miss`；HTTPS origin 经 `upstream(proxy://...)` / `upstream(https-proxy://...)` / proxy chain 时先向上游代理发 CONNECT，再在隧道内对 origin 做 TLS 握手与独立 ALPN，HTTP origin 仍使用 absolute-form 转发；`tls(min=1.2|1.3, ciphers=<list>)` 已映射为 origin rustls protocol versions 与按规则顺序过滤的 aws-lc cipher provider，并刻意不影响 HTTPS proxy hop 自身的 TLS；上游 mTLS 支持同一动作中的 `client-cert=<path>, client-key=<path>`，路径支持规则模板并优先按 storage-relative 解析，可应用于直连 HTTPS origin、SOCKS5 后的 origin TLS、以及上游 proxy CONNECT 后的 origin TLS，trace flags 标记 `upstream-mtls`；trace detail/export/spill 中的 `tls` 数组记录 `phase`、`host`、`handshake_ms`、`peer_certificates`、`protocol`、`cipher_suite`、`alpn` 与失败 `error`，失败握手也保留结构化记录。上游 origin/HTTPS-proxy TLS 握手使用总时长 deadline，默认 10 秒且可用 `--upstream-tls-handshake-timeout-ms` 调整；每次底层读写都只获得总 deadline 的剩余时间，静默或慢速 peer 超时返回 504 并标记 `upstream-tls-handshake-timeout`，证书/协议错误仍返回 502。客户端侧 CONNECT 后的 MITM TLS 握手也使用逐次读写收紧剩余时间的绝对 deadline，默认 10 秒且可用 `--client-tls-handshake-timeout-ms` 调整；静默客户端被主动关闭，trace 记录 408、`client-timeout` / `client-tls-handshake-timeout` 和失败的 `client_mitm_tls`，正常或非超时失败后均恢复 socket 原始读写 timeout。MITM 内层 HTTP/1 session 的 `duration_ms` 从 CONNECT 开始计时，h2 多路复用 session 则从各 stream 请求开始计时。

CONNECT 入口策略现已覆盖 `--no-mitm`、规则 `bypass`、无 CA、自动模式和
`--strict-mitm`。自动模式在回写 CONNECT 200 后以 `peek` 非消费地识别 TLS
ClientHello、明文 HTTP 和未知协议：TLS 进入 MITM，明文 HTTP 回到同一规则/
转发/trace 管道，未知协议或 250ms 默认探测超时进入透传。非超时客户端 TLS
握手失败会写入默认 1024 容量、300 秒 TTL 的 host LRU，客户端下一次 CONNECT
命中 `mitm-fallback-cache-hit` 并透传；strict 模式记录
`mitm-fallback-disabled` 而不写缓存。首次失败连接无法原地降级，因为 200 已发送
且握手字节已消费；自动降级明确以客户端重试为边界。状态 API 回显模式、容量、
活动条目数、TTL 和探测时限。上述路径已有本地 socket 自动化测试，本轮未新增
真实客户端 Dogfooding 轮次。

### 5.4 WebSocket / HTTP2 / SSE

- **WS**：upgrade 请求经过规则管道（可改写握手头）；连接建立后，若无帧级需求 → 字节透传；开启帧 trace（`enable://frameCapture` 或全局开关）→ tungstenite 解帧旁路记录（帧缓存有限队列，默认 512 帧/连接段）。
- **H2**：客户端侧与上游侧独立协商，hyper 自动桥接 h2↔h1；伪头正确映射。
- **SSE**：按 `content-type: text/event-stream` 识别，流式透传 + 按 `\n\n` 切帧记 trace（对齐 whistle data.js 的 SSE 帧逻辑）。

当前可执行子集：HTTP/1.1 WebSocket upgrade 已进入规则管道并支持响应头改写；plain TCP WebSocket 已支持线程化并发双向转发；TLS/MITM WebSocket 已支持单线程非阻塞双向转发，可处理服务端在客户端发送业务帧前先发帧的 wss 会话；decoded frame trace、FIN/opcode 元数据、fragmentation continuation 记录、ping/pong control-frame 透传与记录、binary frame hex preview、`--trace-body-limit` 截断标记、TLS trace 与 zstd spill 中的帧元数据保真均已 dogfood。

当前 H2 可执行子集：客户端 CONNECT MITM 可通过 ALPN 协商 h2，Hyper 1.x 在共享 Tokio runtime 上处理最多 256 个并发 stream；伪头先映射为 request head，DATA/request trailers 再经容量固定的 channel 进入 blocking pool 中既有的有界规则/转发/trace 管道，响应 head、DATA 和 trailers 也通过独立有界 channel 返回，不再在 h2 bridge 边界全量聚合。HTTPS origin 独立协商 `h2` / `http/1.1`：选择 h2 时由 Hyper client 把 h1 或 h2 客户端请求转换为 h2 pseudo headers/data/trailing headers，并把 h2 response data/trailers 送回统一响应期规则管道；选择 h1 时进入共享 Hyper h1 keep-alive 池。因此 h2→h1、h1→h2 与 h2→h2 均已执行和 dogfood，connection-specific headers/`Connection` 声明的扩展头会被剥离，`TE` 仅保留 `trailers`，`--max-header-size` / `--max-header-count` 同时约束上游 h2。上游 h2 连接按 state/origin/route/TLS policy 池化，容量 256，空闲 60 秒淘汰（活动 stream 不视为空闲），trace flags 标记 `h2-upstream`、pool hit/miss；规则切换直连/代理路由会使用不同连接键。每 key 上游 h2 活跃 stream 默认上限 256，可用 `--h2-pool-max-active-streams-per-key` 调整；达到本地上限或等待同 key 冷池 connector 时使用独立 `--h2-pool-wait-timeout-ms`（默认 15 秒），等待时间进入统一 `pool_wait_ms`，超时返回 504。冷池同 key 仅允许一个 connector owner，其余请求等待首个连接发布后在同一 session 上发 stream。远端 `SETTINGS_MAX_CONCURRENT_STREAMS` 若低于本地上限，Hyper 会在其内部 dispatch task 排队，当前计入请求耗时而非 `pool_wait_ms`；需要确定性等待归因时应把本地上限配置为不高于 origin 策略，精确读取远端配额需后续下沉到更低层 h2 dispatch。请求 trailers 已在 h1→h1、h1→h2、h2→h1、h2→h2 四向验证，二进制 unary gRPC frame、`application/grpc`、`grpc-status` / `grpc-message` 与规则追加响应 trailers 也已完成端到端 dogfood。上游 h1/h2 response body 已通过共享有界 frame 通道流式送往 h1.1 或 h2 下游；普通 h1 和客户端 h2 的超限请求体在 origin 协商 h2 时使用有界 streaming request body，cold 与 pool-hit 均支持，ALPN 回落 h1 时使用独占手写链路。h2 stream lease 同时由请求发送和响应 body 持有，双向都结束后才释放本地并发配额。上述新双向流式边界已有真实 TLS+h2 自动化证据但尚未增加 Dogfooding 轮次。Unix 两侧均使用共享 `AsyncFd` readiness 适配器驱动非阻塞 rustls，非 Unix 保留定时回退。SSE 请求和 HTTP/1.1 WebSocket upgrade 会固定 origin h1 以保留既有流式/upgrade 语义；CONNECT over h2、WebSocket over h2 与 h2c 仍按 v2+ 非目标处理。

### 5.5 上游连接管理

- 上游连接池：按 `(scheme, host, port, 代理链)` 维度池化（hyper-util `Client` 定制 connector），每 key 空闲连接上限默认 256（对齐 whistle sockets 配置）、空闲超时 90s。**池等待（checkout）有独立超时（默认 15s）且等待耗时计入 trace 时序**——whistle 池饥饿时请求静默排队、超时无从归因，这里显式暴露。
- 上游代理链：支持 `proxy://host:port`（HTTP 代理）、`https-proxy://`（TLS 到代理）、`socks://`（SOCKS5 上游）三种 operation 指定的转发。

当前可执行子集：HTTP 请求路径已支持 `upstream(proxy://host:port)` / `upstream(http://host:port)`、HTTP 代理多跳链 `upstream(proxy://p1, proxy://p2)`、HTTP/SOCKS/HTTPS-proxy 混合多跳链 `upstream(proxy://p1, socks5://s1)` / `upstream(proxy://p1, https-proxy://hp1)`、多个 `https-proxy://` hop 形成的嵌套 TLS 多跳链 `upstream(https-proxy://hp1, https-proxy://hp2)`、TLS 到代理的 `upstream(https-proxy://host:port)`，以及 SOCKS5 上游 `upstream(socks://host:port)` / `upstream(socks5://user:pass@host:port)`（无认证或用户名密码认证）；`direct` 已支持在 HTTP 请求路径显式覆盖已匹配的 `upstream(...)` 并直连当前目标（保留 `host(...)` 目标改写）；CONNECT 透传链路已支持经 HTTP proxy、HTTPS proxy、SOCKS5 上游代理、HTTP proxy 多跳 chain、HTTP/SOCKS/HTTPS-proxy 混合多跳 chain，以及多个 `https-proxy://` hop 参与的嵌套 TLS 多跳 chain。

普通 HTTP/1.1 上游请求已接入共享 Hyper h1 keep-alive 池：连接键包含运行 state、storage、origin、完整 route、TLS policy/identity 与 header 限制，支持每 key 多个空闲 sender，全局和每 key 空闲上限均为 256，空闲 90 秒由共享 sweeper 淘汰。池化覆盖明文与 TLS origin、直接和既有代理 route；WebSocket、SSE、请求限速及 ALPN 选择 h1 的 HTTP/1.0/超限流式请求保留专用手写路径。小请求体仍使用共享 h1/h2 上游池；大请求体不进入共享 h1 pool，在 origin 协商 h2 时使用有界 streaming request channel 和共享 h2 session，否则使用独占 origin h1 连接，避免复用带有未完成请求体的 h1 连接。每 key 活跃租约默认上限 256（h1 每条连接同一时刻一个请求），可用 `--h1-pool-max-active-per-key` 调整；达到上限后使用条件变量等待，`--h1-pool-wait-timeout-ms` 独立配置等待超时（默认 15 秒）。租约从 checkout 前持续到流式 response body/trailers 完整结束，只有正常 EOF 且 sender ready 的连接才回池；等待成功时 `pool_wait_ms` 写入 trace detail/summary/spill、HAR `timings.blocked` 和 TUI，等待超时返回 504、保留阶段错误并标记 `h1-upstream-pool-wait-timeout`。origin `Connection: close` 或 body 中断不回池；失效 sender 仅在发送前回退新连接，发送开始后的错误不自动重试非幂等请求。trace flags 另标记 `h1-upstream` 和 pool hit/miss。代理会向 h1 origin 声明 `Connection: TE` / `TE: trailers`，并在下游响应前剥离 `Connection` 声明的扩展头及 `Keep-Alive` 等逐跳字段；`status.h1_pool` 回显活跃上限和等待超时配置。

上游 h2 另有跨客户端请求复用的单连接池：连接键使用同一套 state/origin/route/TLS policy/header 隔离，支持并发 stream；失效 sender 在发请求前安全重连，发送开始后的错误不自动重试非幂等请求；全局最多 256 个活跃键，60 秒无访问且无活动 stream 时自动关闭。缓冲与流式请求共用连接池，流式 request body 使用容量 8 的 DATA/trailer/error channel；lease 由 request producer 与 response consumer 共同持有，避免响应头/响应体提前结束时泄漏并发计数。每 key 活跃 stream 默认上限 256，`--h2-pool-max-active-streams-per-key` 可调；本地 checkout 与同 key connector 等待共用独立的 `--h2-pool-wait-timeout-ms`（默认 15 秒），等待成功写入 `pool_wait_ms`，超时返回 504 并标记 `h2-upstream-pool-wait-timeout`。首次连接由 generation token 保证单 key single-flight，连接发布后唤醒等待者并复用同一 h2 session；connector 失败或 ALPN 回退 h1 时释放 owner，后续等待者可继续接管而不泄漏 stream 配额。`status.h2_pool` 回显本地活跃 stream 上限和等待超时。
- DNS：已接入 hickory-resolver；默认读取系统 resolver，`--dns-server IP[:PORT]` 支持重复或逗号分隔的自定义 UDP/TCP nameserver。正/负缓存 TTL 上限默认 60s，可用 `--dns-cache <seconds>` 调整，设为 0 完全关闭缓存；A/AAAA 并行查询且 IPv4 结果优先，避免 IPv4-only 域名的 AAAA NODATA 负缓存遮蔽已缓存 A。`host(...)` 改写为字面 IP 时直接跳过 DNS。`status.dns` 暴露模式、服务器、TTL、lookup/成功/失败/超时/字面 IP 绕过计数。
- **超时分类学**：各阶段独立计时、独立可配，超时错误明确标注属于哪一类（写入响应错误体与 trace 的 `error` 字段）：

  | 阶段 | 默认 | 说明 |
  | --- | --- | --- |
  | DNS 解析 | 5s | 已实现 `--dns-timeout-ms` 总 deadline；解析超时返回 504 并标记 `upstream-dns-timeout`，NXDOMAIN/NODATA 仍为 502；普通 HTTP 与 CONNECT tunnel 同值生效 |
  | 池等待（checkout） | 15s | 普通 h1/h2 已实现，分别用 `--h1-pool-wait-timeout-ms` / `--h2-pool-wait-timeout-ms` 调整；池饥饿返回 504 并显式归因 |
  | TCP 连接 | 10s | 已实现 `--tcp-connect-timeout-ms` 全候选地址共享 deadline；超时返回 504，立即拒绝/不可达仍为 502；普通 HTTP 与 CONNECT tunnel 同值生效 |
  | TLS 握手（上游/客户端侧） | 10s | 上游使用 `--upstream-tls-handshake-timeout-ms` 总 deadline、504 与失败 TLS trace；客户端 MITM 使用 `--client-tls-handshake-timeout-ms` 总 deadline、408、专用 flags 与失败 TLS trace |
  | 首字节（TTFB） | 60s | 已实现 `--upstream-ttfb-timeout-ms`；手写 h1 与流式 origin h2 从请求 body 完成到首个响应头（提前到达的 h2 响应头记 0ms），缓冲 Hyper h1/h2 从 dispatch 到响应头 future 完成，响应 body 明确排除；超时返回 504 并标记 `upstream-ttfb-timeout` |
  | 请求整体 | 360s（对齐 whistle） | 已实现 `--request-timeout-ms` 单调时钟绝对 deadline，并由 `status.timeouts.request_total_ms` 回显；下游 h1 请求体读取/流式转发、请求期规则延迟、池等待/readiness、DNS、TCP、代理链协商、上游 TLS、协议握手、TTFB 和完整缓冲响应体共用同一剩余预算，总时限先耗尽时返回 504、错误为 `stage=request_total` 并标记 `request-timeout` / `request-total-timeout`。CONNECT 透传只约束建链；SSE/WebSocket 在响应头/upgrade 完成后、CONNECT 在隧道建立后解除该 deadline，避免误杀长连接 |
  | 空闲读写 | 可配，默认关 | 流式长连接不误杀 |

---

## 6. 规则路由引擎设计

独立 crate `rsproxy-rules`，**不依赖 tokio/hyper**，输入抽象的 `RequestMeta`/`ResponseMeta`，保证可纯单测与 fuzz。

### 6.1 语法：全新 DSL

**已决策不兼容 whistle 语法**，仅对齐能力。设计原则：

1. **行式规则保留**（grep/diff 友好）：`matcher actions... [when 条件]... [@属性]`；
2. **通配符处处一致**：`*` 不跨段（域名不跨 `.`、路径不跨 `/`、query 不跨 `&`），`**` 跨段，域名/路径/query 三处语义相同——去掉 whistle「加 `^` 才开启路径通配」的开关怪癖；字面 `*` 用 `\*` 转义；
3. **动作命名空间化**：`req.header(...)`、`res.body.replace(...)` 替代 whistle 74 个平铺协议名 + 别名表，可发现性与可读性优先；
4. **条件显式化**：`when 条件`（取反 `when !条件`）替代 `includeFilter://`/`excludeFilter://` 双协议；
5. **一切可解释**：任何规则可被 `rsproxy rules test <url>` explain 到「命中/未命中的具体原因」。

```ebnf
ruleset  := line*
line     := comment | rule | blank
comment  := '#' .*
rule     := matcher WS action (WS action)* (WS when)* (WS prop)*

matcher  := '!'? (regexp | exact | glob | portpat)
regexp   := '/' regex-body '/' flags?              // flags: i；支持命名捕获 (?<name>…)
exact    := '=' url                                 // 完全相等；不写 query 则 query 任意
glob     := [scheme '://'] hostglob [':' portglob] [pathglob] ['?' queryglob]
portpat  := ':' digit+                              // 仅按端口
prop     := '@' ('important' | 'disabled' | 'tag:' name)

action   := name ['(' args ')']                     // name 可带命名空间：req.header
when     := 'when' WS '!'? cond
cond     := name '(' args ')'                       // method/host/header/body/status/ip/chance/url/any
args     := 逗号分隔；值可为 raw | "quoted" | @key | <path> | /re/ | ${var} | $1-$9
```

**matcher 语义表**：

| 写法 | 语义 |
| --- | --- |
| `example.com` | 该域名本身（不含子域），任意协议/端口/路径 |
| `*.example.com` | 一级子域（`a.example.com` ✅，`a.b.example.com` ❌） |
| `**.example.com` | 任意级子域，**含域名自身**（最常用形态，故设计为含自身） |
| `example.com:8080` | 限定端口；不写端口则任意端口 |
| `example.com/api/**` | 路径 glob；无 glob 的路径为 `/` 边界前缀匹配 |
| `https://example.com/p` | 限定协议（http/https/ws/wss/tunnel） |
| `=https://a.com/p?q=1` | 精确匹配（路径与 query 完全相等） |
| `/user\/(?<uid>\d+)/i` | 正则；`i` 忽略大小写；backref/lookaround 自动走 fancy-regex（带预算） |
| `:8443` | 仅按端口匹配（常用于隧道） |
| `!<matcher>` | 排除：命中即令本条之后的同动作规则跳过该请求 |

**捕获传值**：glob 的每个 `*`/`**` 依序绑定 `$1`-`$9`；正则支持编号与命名捕获（`${uid}`）。模板变量沿用 `${url}`、`${host}`、`${method}`、`${reqH.k}` 等（[§6.3](#63-v1-动作action集) 末尾列表）。

**值引用**：`@key`（values 存储）、`<path>`（文件内容，先按 storage 相对路径读取，再尝试原路径）、`"..."`（行内字符串，支持转义与 `${var}` 插值）。`@key` 只允许 1-128 个 ASCII 字母、数字、点、下划线或连字符；文件路径属于受信规则的文件系统能力，不做 storage 沙箱化。

**示例**（等价于 whistle 常用场景）：

```
# hosts 改写（whistle: host://）
api.example.com                  host(10.0.0.7:8080)

# 目录 mock + glob 捕获（whistle: file:// + $1）
**.example.com/api/**            mock(<mocks/$2.json>)          when method(GET)

# 正则捕获注入请求头（whistle: reqHeaders + includeFilter）
/\/user\/(?<uid>\d+)/            req.header(x-uid: ${uid})      when !header(authorization)

# 弱网模拟（whistle: resDelay + resSpeed）
**.slow.example.com              delay(res, 2s) throttle(res, 100KB/s)

# 短路返回 + 概率注入故障（whistle: statusCode + chance filter）
example.com/legacy/**            status(410)
api.example.com/pay/**           status(502)                    when chance(0.1)

# 证书固定的 app 不解密（whistle: ignore + 默认透传）
pinned.example.com               bypass
```

**when 条件语义**：多个 `when` 之间为「且」；一个条件内多值为「或」（`when method(GET, POST)`）；显式「或」用 `when any(cond1, cond2)`；取反 `when !cond`。依赖响应的条件（`status`、`res.header`）延迟到响应期评估（继承 whistle 的两阶段模型）。条件全集：`method`、`host`、`url(glob|/re/)`、`header(k ~ pat)`、`res.header(k ~ pat)`、`body(~ pat)`、`status(pat)`、`ip`/`clientIp`/`serverIp`、`chance(0.0-1.0)`、`env(k=v)`、`any(...)`。

**优先级模型**（继承 whistle 语义）：分组顺序 → 组内文本顺序；`@important` 插队到最前；同一动作族「首条命中生效」，标注为可叠加的动作族（header/replace/append 类）收集全部命中项按序应用；matcher 命中但 `when` 不满足 → 继续尝试下一条同动作规则。

**交付物**：完整 DSL 规范（每个 matcher/action/cond 的正反例、转义规则、错误码）单独成文 `docs/rules-dsl-spec.md`，作为 M1 交付物；测试 corpus 与规范同源（规范中的示例即用例，CI 校验两者一致）。

**whistle 迁移**：v2 提供 `rsproxy rules import --from-whistle <file>`，静态转换高频规则形态（host/file/statusCode/redirect/req\*/res\* 等），无法转换的逐行输出报告。

### 6.2 编译模型（性能核心）

解析产物不是「规则数组」，而是**分层索引的编译规则集**：

```rust
pub struct CompiledRuleSet {
    version: u64,                       // 热更新代数
    // 第一层：按域名精确/后缀索引（覆盖绝大多数规则的快速路径）
    domain_index: HashMap<Box<str>, RuleBucket>,   // "example.com" / "example.com:8080"
    suffix_index: SuffixTrie<RuleBucket>,          // ".example.com" / "**.example.com"
    // 第二层：无法按域名收敛的规则（纯正则/全局通配/端口/排除）
    global_rules: Vec<CompiledRule>,
    // 正则/通配符预过滤：所有 pattern 的必含字面量 → aho-corasick 一次扫描
    prefilter: AhoCorasick,
    prefilter_map: Vec<SmallVec<[RuleId; 4]>>,
    values: ValueStore,                 // {key} 值表（Arc<[u8]>，零拷贝引用）
    rules: Vec<CompiledRule>,           // 全量规则，RuleId 索引
}

pub enum Matcher {
    DomainExact { port: Option<u16> },
    PathPrefix { path: Box<str> },      // `/` 边界前缀
    Exact { path: Box<str>, query: Option<Box<str>> },   // `=` 精确
    Glob(CompiledRegex),                // glob 编译为正则，保留捕获组
    Regex(CompiledRegex),
    Port { port: u16 },
    Negation(Box<Matcher>),
}

pub enum CompiledRegex {
    Linear(regex::Regex),                       // 默认：线性时间
    Fancy(fancy_regex::Regex, ExecBudget),      // backref/lookaround 降级路径
}
```

匹配流程（每请求一次）：

1. 从 URL 提取 `host[:port]`（零分配，借用切片）；
2. `domain_index` 精确查 → `suffix_index` 后缀查（逐级剥离子域名，最多 O(标签数)）→ 得到候选 bucket；
3. bucket 内规则按**动作族分桶 + 文本顺序**排列：非叠加动作族取首个通过 when 条件的规则即停，叠加动作族（⊕）收集全部；
4. `global_rules` 仅当 prefilter（aho-corasick 对全 URL 一次扫描）报告可能命中时才逐条验证正则；
5. when 条件评估：请求期可判的（method/body/ip/header/chance/url）立即判；依赖响应的（status/res.header）延迟到响应期复评；
6. 捕获组：命中含捕获的规则时才执行一次带捕获的正则匹配（`regex` 的 `captures` 慢于 `is_match`，避免无谓开销），结果写入 `MatchedRules` 供 `$1-$9` 与 `${var}` 展开。

复杂度目标：**万级规则下单次匹配 < 10µs（p99）**，域名快速路径 < 1µs。

当前可执行子集：`RuleSet` 已在 parse 阶段构建 exact-domain bucket、suffix-domain bucket、global rule bucket，以及 Aho-Corasick 多 literal regex prefilter；`resolve()` 先按请求 host 合并 exact/suffix/global 候选，再通过一次 Aho-Corasick 扫描加入命中的 regex 候选，去重后按 `@important` 与行号恢复原始优先级，再执行既有 matcher/when/capture 逻辑，保证语义不变。`rsproxy rules stats` 暴露 `domain_exact_entries`、`domain_suffix_entries`、`indexed_rules`、`global_rules`、`prefilter_literals`、`prefilter_rules` 等指标；`rsproxy rules bench` 输出 p50/p99/max，用于 dogfooding 和性能回归观测。10k 规则 dogfood release bench 已达到 p99 < 10µs。

### 6.3 v1 动作（action）集

动作按命名空间组织，右列标注对应的 whistle 能力（保证能力对齐无缺口）。标 ⊕ 的动作族可多条叠加（按规则顺序应用），其余首条命中生效：

| 类别 | 动作 | 对应 whistle |
| --- | --- | --- |
| 路由转发 | `host(addr[, addr…])`（多地址轮询）、`upstream(proxy://h:p[, https-proxy://h:p \| socks5://user:pass@h:p…] \| https-proxy://…)`、`direct` | host、proxy/https-proxy/socks |
| 本地 mock | `mock(value)`（自动 content-type 与目录拼路径、多候选 `\|` 回退）、`mock.raw(value)`（含状态行/头的原始响应）、`status(code)`（短路返回）、`redirect(url [, 301\|302\|307])` | file、rawfile、statusCode、redirect、locationHref |
| URL 改写 | `url.rewrite(from, to)`（glob/正则替换）、`url.query(k=v, -k, …)` ⊕ | urlReplace/pathReplace、urlParams、params |
| 结构化删除 | `delete(pathname.0, urlParams.k, reqHeaders.k, reqBody.profile.secret, reqBody.items[1], resBody.meta.debug, trailer.k)` ⊕ | delete（属性在 parse 阶段编译为 typed `DeleteOp`） |
| 请求改写 | `req.method(M)`、`req.header(k: v \| -k \| k ~ /re/repl)` ⊕、`req.cookie(k=v, -k)` ⊕、`req.ua(str)`、`req.referer(str)`、`req.auth(user:pass)`、`req.type(mime)`、`req.charset(cs)`、`req.body.set/prepend/append(value)`、`req.body.replace(/re/, repl)` ⊕、`req.forwarded(ip)` | method、reqHeaders、headerReplace、reqCookies、ua、referer、auth、reqType、reqCharset、reqBody/reqPrepend/reqAppend、reqReplace、forwardedFor、delete |
| 响应改写 | `res.status(code)`（改上游响应码）、`res.header(...)` ⊕、`res.cookie(...)` ⊕、`res.cors(\* \| 详细参数)`、`res.type(mime)`、`res.charset(cs)`、`res.body.set/prepend/append(value)`、`res.body.replace(/re/, repl)` ⊕、`res.merge(json)` ⊕、`res.trailer(k: v)` ⊕、`attachment([filename])`、`cache(max-age \| off)` | replaceStatus、resHeaders、resCookies、resCors、resType、resCharset、resBody/resPrepend/resAppend、resReplace、resMerge、trailers、attachment、cache、responseFor |
| 内容注入 | `inject(html\|js\|css, value [, prepend\|append\|replace])` ⊕（按响应 content-type 门控） | htmlAppend/jsBody/cssPrepend 等 9 个协议 |
| 流控 | `delay(req\|res, 300ms\|2s)`、`throttle(req\|res, 100KB/s\|1MB/s)` | reqDelay/resDelay、reqSpeed/resSpeed（跨 frame 单调 pacer，受绝对请求 deadline 约束） |
| 控制 | `skip([动作类, …])`（跳过全部或指定类规则）、`bypass`（隧道不解密透传）、`hide`（不记 trace）、`tag(name)`（trace 标记） | ignore/skip、默认透传语义、hide、G |
| TLS | `tls(min=1.2\|1.3, ciphers=<suite[:suite...]>, client-cert=<path>, client-key=<path>)`（origin TLS 版本/套件约束与可选 mTLS，可分别或组合使用） | tlsOptions/cipher、G://clientCert |

`reqBody.*` 支持 JSON 和 urlencoded form，`resBody.*` 支持 JSON 与 JSONP；dot
key、转义特殊字符和尾部 `[index]` 均在 parser 中编译为最多 128 段的 typed path。
运行时按 Content-Type 门控，压缩、非法 JSON/UTF-8 或不存在的路径保持原 body，
并复用请求/响应已有的 bounded body planner 与超限降级语义。

**延后（v2+）**：`pipe`、`sniCallback`、模板引擎 mock（tpl/jsonp/dust）、写文件（reqWrite/resWrite）、`log`、`weinre`、`style`、`pac`、脚本/插件类全部能力。

模板变量 v1 支持核心 20 个：`${id}` `${now}` `${random}` `${randomUUID}` `${url}` `${host}` `${hostname}` `${port}` `${path}` `${pathname}` `${query}` `${search}` `${method}` `${clientIp}` `${serverIp}` `${statusCode}` `${reqH.k}` `${resH.k}` `${reqCookies.k}` `${resCookies.k}`，以及 `${var.replace(/re/, repl)}` 变换。

当前可执行子集已覆盖上述 20 个变量：每个 `RequestMeta` 持有稳定的 id/time/random/UUID 快照，响应期动作共享一次 `Arc<ResponseMeta>`，header/cookie 查询分别按 HTTP 头大小写不敏感和 cookie 名大小写敏感处理。模板 replace 在规则发布前验证，运行时使用容量 128 的线程本地正则缓存；正则 matcher 的 `$0` 表示完整匹配。`rules test --response-status/--response-header` 可在在线 API 和离线 fallback 中演练响应变量及响应期条件。

动作参数中的文本来源已统一为 `Value::{Inline, File, Reference}`，parser 在 AST
边界完成分类，代理侧由 `proxy/transforms/values.rs` 集中读取和渲染。文本动作对
非 UTF-8 输入返回明确错误，body/inject/mock 保持二进制字节；外部 UTF-8 内容
在执行时支持 20 个模板变量、编号捕获和命名捕获。URL 正则替换使用独立 raw
解析路径，保留 `$1`/`${name}` 给替换正则。解析期与运行期均验证 value key，
避免程序化 AST 绕过规则 parser 后形成 values 路径穿越。

### 6.4 规则组织与热更新

- **多分组**：Default + 命名分组（对齐 whistle Rules 分组），存储于 `~/.rsproxy/rules/*.rules` 纯文本文件；分组有启用状态与顺序，合并语义与 whistle 一致（靠前分组优先）。
- **values**：`~/.rsproxy/values/` 目录，每个 key 一个文件；大文件懒加载 + mmap。
- **热更新**：CLI/API 写入 → 解析校验（失败整体拒绝并返回行级错误）→ 编译新 `CompiledRuleSet` → `ArcSwap::store`。同时支持 `--watch` 监听文件变更（notify crate）。进行中的请求继续使用旧快照，天然一致性。
- **规则调试**：`resolve()` 返回结果携带「每条命中规则的分组/行号/原文」，支撑 `rsproxy rules test <url>` 的 explain 输出。

当前可执行子集：`RuleStore` 从 `rules/*.rules` 加载 Default + 命名分组，
`rules/groups.toml` 持久化顺序与启停状态；没有 manifest 的旧 storage 会以
default 优先、其余文件名字典序自动发现。CLI/API 已支持
`ls/cat/edit/set/rm/enable/disable <group>`，`rules test/stats/bench` 使用完整
启用集合。更新先校验所有分组并编译单一索引，再通过 ArcSwap 一次发布；每个
请求从 body 计划到响应期复评持有同一旧/新快照。`--watch` 或 TOML
`watch = true` 会监听 `*.rules` 与 `groups.toml`，默认 200ms trailing-edge
debounce 可通过 `--watch-debounce-ms`/`watch_debounce_ms` 调整。回调使用容量
64 的有界 try-send 通道并过滤无关事件；worker 每批整目录读取、校验全部分组，
成功后一次发布，非法编辑继续使用旧快照。API 写入造成的重复事件按分组内容
去重；status 暴露 event/drop/reload/failure、最后成功时间和最后错误。
当前 values 读取为按需 `fs::read`，尚未实现上述大文件 mmap/cache 目标。

---

## 7. 请求 Trace 设计

### 7.1 数据模型

```rust
pub struct Session {
    pub id: u64,                        // 单调递增
    pub kind: SessionKind,              // Http | Tunnel | WebSocket | Sse
    pub timings: Timings,               // start/dns/pool_wait/connect/tls/req_end/ttfb/end (µs)
    pub client: SockAddrInfo,           // ip:port
    pub server: Option<SockAddrInfo>,   // 实际连接的上游 ip:port
    pub req: MessageRecord,             // 方法、URL、http 版本、头、trailers、body 句柄
    pub res: Option<MessageRecord>,     // 状态码、头、trailers、body 句柄
    pub rules: Box<[MatchedRuleBrief]>, // 命中规则（协议/原文/分组/行号）
    pub frames: Option<FrameRing>,      // WS/SSE 帧（有限环）
    pub tls: Box<[TlsTraceBrief]>,       // TLS 阶段、host、耗时、证书链数量、协议/套件/ALPN、失败错误
    pub flags: SessionFlags,            // 截断/透传/mock/错误/被忽略...
    pub error: Option<Box<str>>,        // 超时类错误标注阶段（如 "timeout: pool_wait 15s"）
}

pub enum BodyHandle {
    Inline(Bytes),          // 小 body，内存
    Spilled { seg: SegmentId, offset: u64, len: u32, zstd: bool },  // 落盘
    Truncated { head: Bytes, total: u64 },   // 超限截断（保留前缀）
    Skipped { reason: SkipReason, total: u64 }, // 未采集（策略/透传），只有大小
}
```

记录的详细度对齐并超过 whistle：完整时序含 DNS/TLS 细分、上游真实 IP、命中规则带行号溯源、h2 伪头、请求/响应 trailers、WS 帧方向与掩码信息。

当前可执行子集已在原生 JSON、摘要、NDJSON spill、TUI 与 HAR 中贯通
`pool_wait_ms`、`dns_ms`、`connect_ms`、`request_send_ms`、`ttfb_ms` 和
`response_receive_ms`。两个传输边界使用 `Option<u64>`，明确区分真实 0ms 与
mock/旧记录/尚未到达边界；`TraceEvent::End`、pending 聚合和 spill 均保留该
语义。Hyper h1/h2 请求体由单调 one-shot timer 包装，在 EOF 或提前 drop 时冻结；
流式 h2 上传从首个 dispatch 到 body/trailer 关闭独立计时。h1/h2 response pump
从响应头到 body/trailer EOF 或错误共享另一个 timer，手写 h1 路径在实际写入和
读取边界显式打点。响应头超时会保留已完成的 request-send 且保持 receive 未知，
响应体错误会保留截至错误的 receive。

HAR 将三段投影到顺序的 `timings.send/wait/receive`，上游 TLS 握手映射到
`timings.ssl`；h2 双向流式允许 send/receive wall-time 重叠，标准 `receive` 因此
截到剩余顺序预算，不能承载的重叠量写入扩展 `transfer_overlap_ms`。已知传输边界
之外的规则处理等剩余时间归入标准 `blocked`，保证标准 timing 总和仍与 `time`
闭合。`_rsproxy.timings` 无损保留 nullable 原值、`boundaries_complete`、
`transfer_overlap_ms` 和 `unattributed_ms`。HAR `startedDateTime` 使用 RFC 3339
UTC，客户端 h2 写 `HTTP/2`，重复 query 参数按顺序解码到 `queryString`；扩展还
保留 session id、client/upstream、flags、error、rules、TLS records 与 frame count。
客户端 h2 的 MITM 握手只作诊断，不从 stream 请求时间线重复扣除。

### 7.2 采集管道（不碰热路径）

```
proxy task ──try_send(TraceEvent)──▶ bounded mpsc(容量 8192)
                                        │  满则丢弃 + dropped 计数器（宁丢 trace 不阻塞代理）
                                        ▼
                              collector task（单消费者）
                                 组装 Session / 截断 body / 写环形缓冲
                                 按策略 spill 到磁盘段文件
                                 发布给 follow 订阅者(每订阅者独立有界队列)
```

- 事件粒度：`Start`、`ReqHeaders`、`ReqBodyChunk`、`ResHeaders`、`ResBodyChunk`、`Frame`、`End(timings)`。body chunk 传 `Bytes` 引用，**零拷贝**（与转发共享同一块缓冲）。
- 采集全程只在 collector 单线程聚合，无锁竞争。

当前可执行子集：`TraceStore` 是 cloneable facade，公开 `Start`、`Request`、
`Response`、`BodyChunk`、`BodySnapshot`、`Frame`、`Tls`、`End`、`Abort` 事件。
所有 producer 只做原子 id 分配、字节预算预留和容量 8192（可用
`--trace-queue-capacity` 调整）的 `try_send`；队满、超过队列字节预算或 collector
退出时立即返回并累计丢弃计数，代理线程不获取 collector Mutex，不序列化、压缩
或写盘。单消费者在 `store/pending.rs` 聚合 metadata/body 事件，body preview 受
硬上限约束，5 分钟未结束会话自动记为 incomplete；`BodySnapshot` 在收尾时覆盖
增量计数，因此中途 chunk 丢失不会造成最终已完成 session 的字节数或前缀漂移。

HTTP 请求在规则、tag 与 hide 决策后立即发送 `Start`/`Request`；流式 h1/h2 上传、
池化 h1/h2 响应和手写 h1 SSE 在传输过程中发送 body 事件。h2 路径复用 `Bytes`
切片，h1/SSE 只复制尚未达到上限的 preview；预览耗尽后仍发送空 data + 实际
`observed_bytes`，保持总量和 queue 预算准确。成功收尾以一个原子 continuation
batch 发送最终 metadata、双向 snapshot、frame/TLS 与 `End`；错误路径由 guard
发送幂等 `Abort`。passthrough CONNECT/tunnel 在规则与 `hide` 决策后使用同一
Start/Request/Response/body/End 生命周期；普通 TCP 与 TLS-to-upstream copy 都按
方向发送空 `Bytes` preview + `observed_bytes`，避免保存不透明 payload，最终
snapshot 校正队列丢弃。连接拒绝、MITM 握手超时和空连接也通过事件 continuation
收尾。代理生产路径不再调用兼容 `record(Session)`；该 API 只作为公开兼容入口保留。

list/get/stats/clear/export 与事件共享 FIFO request/reply 屏障；最后一个 store
句柄析构会刷完此前接受的命令并 join collector。`follow` 在同一 FIFO 中原子取得
backlog 并注册有界 live subscriber，慢订阅者只丢自己的记录且不阻塞 collector；
follow handle 持有强 liveness token，worker 只保留弱引用，因此客户端退出后的
下一次 stats 会立即清理订阅者，不必等待下一条 session。控制端将正常 BrokenPipe/
reset 记为 debug，真实请求故障仍为 WARN。

### 7.3 资源控制（核心需求）

| 维度 | 默认 | 机制 |
| --- | --- | --- |
| 会话条数 | 4096 条 | 内存环形缓冲，满则最旧的整体驱逐（含其 inline body） |
| 单 body 采集上限 | 512KB（可调 `--trace-body-limit`） | 超出保留前 64KB + 总长标记 `Truncated` |
| 内存总预算 | 256MB（`--trace-mem-budget`） | 计量所有 inline body + 头估算；超预算触发提前驱逐/更早截断 |
| CPU | 近零 | 采集仅 clone `Bytes` 引用 + try_send；解压/格式化延迟到查询时按需做 |
| 磁盘 | 默认关闭；开启后上限 2GB（`--trace-disk-budget`） | 段文件轮转删除 |
| 采集策略 | 全采集 | `--trace-filter` 支持仅头模式（`headers-only`）与媒体 body 预览排除（`media`，默认不采 image/audio/video/font 的 body）；后续补更多过滤策略；`hide` 动作完全跳过 |
| 背压保护 | mpsc 满即丢 | `rsproxy trace stats` 暴露 dropped 计数，提示用户收紧策略 |

当前 256MB 总预算同时覆盖排队事件与 collector resident 数据，可通过
`--trace-mem-budget` / TOML `trace_mem_budget` 调整。默认固定划分为 64MB queue
和 192MB resident；非默认预算自动给 queue `min(total/4, 64MB)`，其余给 pending
+ completed session。queue 以事件容器 capacity、字符串、body `Bytes` 和
`observed_bytes` 估算并在 `try_send` 前原子预留，worker 处理后释放；resident 按
String/Vec 实际 capacity 计入 header、preview、rules、frames 和 TLS。pending 超限
会中止当前 partial session，completed 环超限按最旧整体驱逐；完整 session 可先
spill，再受 resident 驱逐。

`GET /api/trace/stats` 与 `/api/status.trace` 公开 queue/resident/total 当前字节和
预算、`queue_memory_dropped`、pending/completed 字节、pending/incomplete/orphan、
follow subscribers/dropped、环驱逐及 spill 状态；兼容字段 `dropped` 与
`queue_dropped` 同义。该机制已有并发、乱序、队满、慢 follower、partial 超预算
和最终 snapshot 校正测试。`scripts/verify.sh stream` 会启动 release
代理与真实 TCP origin/client，默认传输 1GiB、以固定 64KiB 缓冲解码下游合法的
Content-Length 或 chunked body，并核对 trace 总字节、4KiB preview、queue drop、
partial 和总内存预算。2026-07-12 最新运行传输 1,073,741,824 bytes，用时 679ms，
RSS 从 15,328KiB 到峰值 18,336KiB，增长 3,008KiB，低于 96MiB 门槛。

Loop 96 用 release daemon、真实 CLI 和 curl 验证了 HTTP 与 no-MITM CONNECT/TLS；
live follow 同时观察 1,024-byte HTTP session 和双向 tunnel 字节，stats 无 drop、
partial、orphan 或 spill error，zstd spill snapshot/JSON export 含两条，HAR 只含
HTTP。TUI 显示独立 timing，replay 返回精确 body；单条 follow 退出后无需额外
publish 即显示 `follow_subscribers=0`，且默认 info 日志不再出现正常断连 WARN。

### 7.4 磁盘落盘（spill）

- 结构：`~/.rsproxy/traces/seg-{n}.rst` append-only 段文件（默认 64MB/段）+ `seg-{n}.idx` 稀疏索引（session id → offset）。
- 写入：collector 批量 buffered write（flush 间隔 200ms 或 1MB），可选 zstd level 1 压缩（大 body 收益高、CPU 低）。
- 淘汰：按段整体删除（无碎片、无 compaction），保留策略 = min(段数上限, 磁盘预算)。
- 崩溃安全：段尾带 CRC 帧界，恢复时截断到最后完整帧即可（trace 属可丢数据，不做 WAL）。

当前可执行子集：collector 会把每个已接受会话追加为 NDJSON 段文件到 `<storage>/trace/seg-{n}.ndjson`，并为每段维护 `<storage>/trace/seg-{n}.ndjson.idx` sidecar 索引（offset、length、session id、CRC32）；代理请求线程不执行这些 I/O。`--trace-filter headers-only` 会保留请求/响应头、状态、字节数和规则命中，但不采集请求/响应 body preview；`--trace-filter media` 会对 `image/*`、`audio/*`、`video/*`、`font/*` 与常见 font application MIME 保留头和字节数但不采集 body preview（默认开启，`--trace-filter full` 可关闭）；`--trace-spill-compression zstd[:level]` 会改写为 `<storage>/trace/seg-{n}.ndjson.zst`，每条记录独立压缩为 zstd frame，索引记录压缩帧 offset/length 与原始 JSON payload CRC32；`--trace-segment-size` 控制段大小，`--trace-disk-budget` 控制按最旧段整体删除的磁盘预算，`GET /api/trace/stats` 暴露 `spilled`、`spill_dir`、`spill_bytes`、`spill_segments`、`spill_evicted_segments`、`spill_errors`、`last_spill_error`、`spill_index_entries`、`spill_corrupt_records`、`spill_compression`。`GET /api/sessions/spill.ndjson` 先通过有序命令取得已打开段/索引句柄及不可变长度边界，collector 随即继续处理 event/follow/query；查询线程再执行 CRC 校验、zstd 解压、损坏记录跳过和结果拼接。快照不包含之后 append 的记录，并在 `trace clear` 或预算驱逐删除路径后仍可读；clear generation 会拒绝旧快照的过期 corruption 回写。`trace clear` 仍作为有序 collector 命令同时清空内存环并删除段文件和索引。

### 7.5 查询与导出

控制 API（供 CLI/TUI/未来 Web UI）：

- `GET /api/sessions?after=<id>&limit=&url=&method=&status=&kind=&since=`：游标分页 + 服务端过滤；
- `GET /api/sessions/{id}`：完整详情（body 按需解压/解码，支持 range）；
- `GET /api/sessions/follow`（长连接 NDJSON 流）：实时推送摘要，支持同样的过滤参数；
- `POST /api/sessions/clear`、`GET /api/trace/stats`（条数/内存/磁盘/丢弃计数）；
- 导出：HAR 1.2（RFC 3339、标准 timing + `_rsproxy` 无损诊断扩展）与 rsproxy 原生 JSON（无损）。

当前 `/api/sessions/follow` 是 close-delimited NDJSON 长连接：订阅建立与 backlog
游标读取共享 collector FIFO，随后推送完成 session；空闲时发送可配置 heartbeat
空行。每个订阅者有独立有界队列，慢消费者只增加 `follow_dropped`。CLI
`trace follow` 使用逐行流式 reader，忽略 heartbeat，`--count` 达到后主动断开；
旧 `/api/sessions.ndjson` 拉取端点继续保留兼容。

---

## 8. 控制平面与 CLI 设计

### 8.1 运行模式

- `rsproxy run [opts]`：前台运行（开发/容器）；
- `rsproxy start / stop / restart / status`：daemon 模式，pidfile 与控制端点存于运行配置；Unix 可用 domain socket，Windows 可用 named pipe；`status` 展示端口、规则分组状态、trace 统计、内存预算、版本。
- 配置来源优先级：CLI 参数 > 配置文件（`~/.rsproxy/config.toml`，`--config` 覆盖）> 默认值。所有 whistle 常用启动项有对应参数：`-p/--port`、`--http-port/--https-port/--socks-port`、`--host`、`-t/--timeout`、`-s/--sockets`、`--dns-cache/--dns-server`、`--storage`（多实例隔离目录）、`--no-mitm`/`--strict-mitm`、`--proxy-auth`、`--trace-*` 等。

当前可执行子集：已通过强类型 `serde`/`toml` 模型实现 CLI > 配置文件 > 默认值合并。默认文件跟随默认 storage（`RSPROXY_HOME/config.toml` 或 `~/.rsproxy/config.toml`），缺失时忽略；`--config` 显式文件缺失、TOML 类型错误、未知字段和非法零值均直接报错。容量字段接受字节整数或 `b/kb/mb/gb` 字符串，DNS server 数组、请求/响应 body 聚合上限、trace filter/compression/queue/memory budget、pool/TLS/TTFB/request deadline、proxy/API auth 和规则文件 watch 等当前运行参数均可配置。daemon、status、rules、values、trace、replay、CA、TUI 与系统代理命令复用同一解析路径；TCP/Windows pipe 客户端 token 优先级为 CLI > 环境变量 > TOML > storage token 文件。`status.version`、`status.config`、`status.body_buffer_limit` 和 `status.rule_watch` 回显版本与实际配置，secret 不回显。daemon 子进程在常驻前同步绑定 proxy/control listener，任一 listener 退出会终止进程；启动超时回收 child/pidfile，stop 在终止前以已鉴权 status 校验 storage identity，避免 PID 复用误杀。配置文件本身仍是命令启动时快照，不做热重载；可选 watcher 只重载规则目录。Loop 94 已用 release daemon 验证显式 TOML 启动、CLI 覆盖和 restart 后规则保留；Loop 95 复用 TOML 的 1MB 上限验证请求体流式降级；Loop 96 已真实运行 status/rules/trace/TUI/replay。M4 黑盒矩阵进一步覆盖 restart 规则保留、异常 kill、损坏 pidfile、bind 失败与身份防误杀。

CLI 现仅保留 clap typed 参数/config 合并、daemon listener/readiness 编排与
human/JSON 呈现；
PID 解析、detach、存活检查、终止以及 Unix control-socket 路径组装均通过
`rsproxy-platform::process` 的强类型入口执行。根 CA 生命周期/存储/系统 trust 与
system-proxy plan/execute 也由 platform facade 提供，CLI 适配器不再持有 OS 实现。
net/engine/control/platform/rules 分别公开 `NetError`、`EngineError`、
`ControlError`、`PlatformError` 与 `RuleModelError`，跨 crate 与底层 I/O 错误保留
source；CLI 只以 `CliError` 聚合。`main.rs` 是唯一错误呈现点：运行时失败退出 1，
clap 用法错误退出 2，daemon 状态冲突退出 3；解析成功后使用 typed `json` 字段，
只有 clap 解析失败时才检查原始 argv 中的 `--json`。

### 8.2 控制 API 传输

- 默认 **unix domain socket**（`<storage>/run/ctl.sock`，权限 0600，天然免鉴权）；若
  storage 路径超过保守的 `sun_path` 预算，则确定性回退到 `/tmp` 下 UID+storage-hash
  短路径，并在 stop/失败启动时清理；
- Windows 默认 **named pipe**（`pipe:rsproxy-control`），首实例独占、拒绝远端客户端并使用 storage token 鉴权；
- 可选 `--api 127.0.0.1:8900` 开 TCP（为未来 Web UI / 远程管理），开启时强制 token 鉴权（`--api-token` 或自动生成写入文件）；
- 协议：HTTP/1.1 + JSON；control crate 持有小型本地阻塞 wire，实现不依赖
  `rsproxy-net`。

当前可执行子集：Unix control socket 创建后强制 0600，使用本机 peer/文件权限模式，不发送 token；TCP 与 Windows named pipe 在路由分发前强制认证所有读写端点（包括 status），支持 `Authorization: Bearer` 与 `X-Rsproxy-Token`，使用不提前退出的字节比较，失败返回 401 和 Bearer challenge。named pipe 以拥有 Win32 handle 的同步 `Read + Write` adapter 同时服务 control router 与 CLI client。`--api-token` 可显式配置且至少 16 bytes；未配置时首次启动通过系统 CSPRNG 生成 256-bit token，以 64 字符 hex 写入 `<storage>/run/api-token`，Unix 文件强制 0600，后续前台/daemon 重启复用。CLI token 发现优先级为 `--api-token` > `RSPROXY_API_TOKEN` > storage token 文件，适用于 status/rules/values/trace/replay/TUI；`status.api_auth.mode` 仅回显 `token` 或 `peer`，不暴露 secret。daemon 父进程在 spawn 前准备 token，子进程和 readiness probe 复用同一文件。

上述传输、路由、token auth、JSON/HAR shape 与请求客户端现集中在独立
`rsproxy-control` crate：`server/` 负责 TCP/Unix/Windows pipe 与资源路由，
`client/` 负责普通请求和 NDJSON follow，`shapes/` 负责稳定输出投影。
`ControlOptions` 只携带控制端点及展示元数据，`ControlState` 组合这些 options、
`EngineHandle` 与 Trace handle；status、rules、replay 分别经
`EngineHandle::{status_snapshot,rules,replay}` 的强类型边界进入 engine，control
不读取 `SharedState` 私有字段，也不借用数据面 HTTP wire。CLI 仅保留参数/配置
优先级、命令编排和 TUI。

### 8.3 CLI 子命令全集（覆盖 whistle Web UI 功能）

```
rsproxy run|start|stop|restart|status            # 生命周期
rsproxy rules ls                                 # 分组列表与启用状态
rsproxy rules cat|edit|set|rm <group>            # 查看/$EDITOR 编辑/写入/删除分组
rsproxy rules enable|disable <group>             # 启停分组
rsproxy rules check [file]                       # 语法校验（行级错误+建议）
rsproxy rules import --from-whistle <file>       # whistle 规则静态转换（v2）
rsproxy rules test <url> [-X GET] [-H k:v]... [--client-ip IP] [--response-status CODE] [--response-header k:v]... # 匹配演练：打印命中规则/来源行/最终动作(explain)
rsproxy values ls|cat|set|rm <key>               # values 管理
rsproxy trace ls [--url --method --status -n|--limit] # 会话列表（表格，含 id/方法/状态/耗时/大小/规则标记）
rsproxy trace get <id> [--body req|res] [--raw]  # 会话详情（头+体，自动解压/高亮 JSON）
rsproxy trace follow [过滤参数]                    # 实时跟踪（类 tail -f，Ctrl-C 退出）
rsproxy trace export [--har|--json] [-o file]    # 导出
rsproxy trace clear / trace stats                # 清空 / 资源统计
rsproxy replay <id> [-H k:v --body file]         # 重放请求（composer 等价）
rsproxy ca init|install|uninstall|export|status  # 根证书管理
rsproxy proxy on|off|status [--platform ...]     # 系统代理：macOS/Windows/Linux 原生后端与 dry-run
rsproxy tui                                      # ratatui 全屏实时界面
rsproxy completions <shell>                      # 补全脚本
```

- 输出规范：人读默认表格/彩色；所有查询类命令支持 `--json`（机器可读，供脚本化）；宽度自适应，`NO_COLOR` 遵循。
- `rsproxy tui`：左侧会话流（实时滚动、过滤输入框）、右侧详情 Tab（Headers/Body/Timing/Rules）、`r` 重放、`/` 过滤 —— 覆盖 whistle Network 面板核心操作。

当前可执行子集：`rsproxy tui` 已作为控制 API 客户端落地，使用 ratatui + crossterm 渲染全屏实时界面，包含 daemon 状态、proxy/API/storage、trace 计数、spill compression、最近会话表、选中会话详情、`/` 过滤输入、`--filter` 初始过滤、overview/headers/body/rules 详情 Tab、`--tab` 初始 Tab、Tab/BackTab 切换、`r` 重放选中会话、`R` 刷新、上下键选择，以及 `q`/Esc 退出；`--once` 提供非交互快照输出，便于脚本和 dogfooding。

`cli/command.rs` 用 `clap` derive 单源定义 13 个顶层与 30 个嵌套命令；
`-h/--help` 和 version 在配置加载、token 发现、daemon/API/platform 操作前由 clap
处理。真实二进制测试逐项覆盖全部层级，证明帮助快速成功退出且不创建 storage 或
监听器；未知、拼错或与命令无关的参数按 clap 惯例以用法错误退出 2。
`clap_complete` 从同一命令树生成 Bash/Zsh/Fish/PowerShell；`--dns-server`、
`-H/--header` 与 `--response-header` 以 append 语义保持重复值及顺序。查询 JSON shape、
`rsproxy.cli.error/v1` 单文档错误、完整 daemon 恢复矩阵和按 offline/online 拆分的
CLI 产品路径均已有专用黑盒测试，M4 对应缺口已关闭。

当前 rules CLI 可执行子集：分组命令已支持在线 API 与离线 storage fallback，
`ls --json` 暴露顺序、启停和规则数。`rsproxy rules test <url> [-X METHOD]
[-H 'Name: value']... [--body TEXT|-d TEXT] [--client-ip IP] [--server-ip IP]
[--response-status CODE] [--response-header 'Name: value']...` 使用完整启用分组，
并把请求元数据及可选响应快照注入与真实代理相同的模板/条件解析路径；未提供
响应参数时保持请求期 explain 语义。响应期 `status`/`res.header` 条件仍在真实
代理响应期复评并写入 trace。
`rsproxy rules bench` 使用同一分组快照与请求元数据，其中 env 条件读取 bench
CLI 进程环境。

当前内容注入可执行子集：`inject(html|js|css, value[, append|prepend|replace])` 已支持按响应 `Content-Type` 门控的 buffered response body 注入；支持 inline、`<file>`、`@key` value、模板变量与多条叠加，注入后自动更新 `Content-Length`。SSE 响应遇到 inject 时走完整 body 路径而非流式直通。

当前控制动作可执行子集：`skip(family...)` 已支持跳过后续指定 action family；`skip()`/`skip(all)`/`skip(*)` 跳过后续全部 actions；skip 自身保留在 explain/trace 中。`hide` 已支持请求期与响应期 trace 抑制，仍会执行同规则的其他改写动作；`tag(name)` 已支持模板渲染后写入 trace flags（`tag:<name>`）。

当前本地 mock 可执行子集：`mock(value)` 已支持 inline、`<file>`、`@key` 与模板变量短路响应；`mock(<a|b>)` 已支持文件多候选 fallback；`mock(<dir>)` 已支持按请求 URL path 拼接目录文件，目录路径 `/` 或以 `/` 结尾时回退 `index.html`；文件 mock 会按最终命中路径扩展名自动推断 `Content-Type`。`mock.raw(value)` 已支持含 HTTP 状态行、响应头、空行和 body 的原始响应短路，并写入 trace 响应头/body。

当前 system proxy 可执行子集：`rsproxy proxy status|on|off` 支持 `--platform macos|windows|linux` 与统一 JSON/dry-run plan；macOS 使用 `networksetup`，Linux 使用 GNOME `gsettings` 并在部分失败时逆序恢复已保存值，Windows 写入当前用户 Internet Settings registry、失败恢复三个 proxy value，并调用 WinINet settings-changed/refresh。CA trust 同样分为 macOS `security`、Linux p11-kit `trust anchor` 与 Windows 当前用户 Root store `certutil`。Windows GNU 全 target check、warning-denied Clippy 和 release link 已通过；Linux/Windows 目标 OS 运行不属于当前 v1 验收范围。

上述 OS 边界现集中在无 rsproxy 内部依赖的 `rsproxy-platform` 叶子 crate。
`ca` 提供根 CA 生成/指纹、类型化路径/状态、PEM 读取、leaf material 持久化与
trust install/uninstall outcome；MITM 叶证书的密码学签发仍由 engine facade 的
`issue_leaf_certificate` 完成。`system_proxy` 以 `ProxyPlatform`、`ProxyAction`、
`ProxyOptions`、`ProxyPlan`、`ProxyOutcome` 以及
`plan_system_proxy`/`execute_system_proxy` 分别表达 dry-run 计划和执行结果；
`process` 提供 daemon/process 原语和长 storage 路径的确定性 Unix socket
fallback。workspace 仍默认 `unsafe_code = "deny"`；platform 仅因 Unix/Windows
process FFI 与 Windows WinINet 通知采用带说明的 crate 级 allow，unsafe 调用局限于
`process.rs` 和 `system_proxy/windows.rs`。

---

## 9. 性能设计

### 9.1 热路径原则

1. **转发路径零拷贝**：`Bytes` 引用计数流转；透传隧道 `copy_bidirectional`（后续评估 splice/io_uring 专项优化）；
2. **热路径无锁**：规则读 `ArcSwap`（wait-free 读）；trace 出口 try_send；证书/DNS 缓存 moka（分段锁，读多写少）；无全局 Mutex；
3. **每请求零/低分配**：`RequestCtx` 复用 SmallVec/内联缓冲；URL 解析借用不复制；匹配结果 `EnumMap` 栈上分配；
4. **惰性计算**：捕获组、模板展开、trace body 解压、JSON 格式化全部推迟到实际需要处；
5. **规则匹配分层短路**：域名索引 → 前缀树 → aho-corasick 预过滤 → 正则验证，正则是最后手段。

### 9.2 关键参数

| 项 | 默认 |
| --- | --- |
| worker 线程 | CPU 核数（tokio 默认） |
| 上游池 per-key 空闲连接 | 256 |
| 叶子证书缓存 | 1024 张 / TTL 24h |
| DNS 缓存 | 尊重 TTL，上限 60s |
| body 改写聚合上限 | 8MB |
| trace 通道容量 | 8192 事件 |
| 单请求 header 总量 / 条数上限 | 256KB / 256 条（客户端侧与上游侧、h1 与 h2 同值；`--max-header-size` / `--max-header-count` 可调，超限 431） |

### 9.3 本机性能目标（验收线，criterion + oha 压测）

| 指标 | 目标 |
| --- | --- |
| 纯转发吞吐（h1, keep-alive, 1KB 响应） | 本机基线 45,392 rps；发布下限为基线的 90%（40,853 rps）；≥ whistle 同机 10 倍 |
| 代理附加延迟 p50 / p99（无规则命中） | < 0.3ms / < 2ms |
| 规则匹配（10k 混合规则，含 20% 正则） | p99 < 10µs |
| TLS MITM 新建连接（证书缓存命中） | 附加 < 3ms |
| 常驻内存（空载 / 4096 会话满 trace） | < 30MB / < 预算上限 |
| 大文件（1GB）代理传输 | 内存平稳（流式，不随文件大小增长） |

当前 M5 驱动已固化为 `benches/{criterion,e2e,soak}/` 的版本化 JSON 报告及严格
checker。Apple M1 Pro 本地 oha 的最佳观测为 54.3k rps；正式 16 并发基线报告为
45,392 rps、direct 101,707 rps、附加 p50 169.25µs、p99 869.29µs、空载
17,952KiB。Whistle 2.10.5 使用性能更有利的 `pureProxy` 模式完成 10k/10k 精确请求、
594.0 rps，rsproxy 同机为 76.4 倍。本机吞吐回归线、延迟、空载 RSS 与 Whistle 10x
均已通过。原 80k/通用 8c 指标不再作为当前 v1 验收线；本机检查通过
`RSPROXY_PERF_MIN_RPS=40853` 使用同一 checker，且性能报告仍需记录 direct 结果以
识别明显的整机负载噪声。

Criterion 当前收集 11 个 rules/trace/certificate 指标；10k 混合规则约 0.99µs，
缓存 TLS 握手置信上界 0.300ms，且相对上一份本机报告无指标回退超过 10%。最新 1GiB
release 验收完成精确 1,073,741,824 bytes，用时 679ms，RSS 从 15,328KiB 到
18,336KiB，仅增长 3,008KiB。最新短时 soak 在 500 QPS、32 并发、1,001 条混合规则
和 trace 开启时完成 5,001 个请求，成功率 100%；RSS 峰值增长 7,360KiB、结束增长
6,288KiB，FD 峰值增长 35、结束增长 3，trace 无丢失或残留。正式高效稳态阶段又
持续 6,307 秒，覆盖 6,379,936 个 session 与 106 个分钟样本；RSS 峰值仅 +6,160KiB，
全程及后半段斜率均为负，FD 峰值 136/上限 144，Trace 无 pending/incomplete/orphan/
drop/spill error。该口径同时覆盖足够的 wall-clock 定时周期、请求量和稳态斜率，
替代低信息密度的机械 24 小时等待。

`.github/workflows/performance.yml` 可在同一 runner 顺序运行 base/current Criterion，
但 hosted 结果只作为附加信息，不属于 v1 验收。正式发布判断以当前 macOS 开发机的
本地报告、10% 回归线和高效稳态 soak 结果为准。

---

## 10. 测试策略

总原则：**规则引擎是正确性核心，用「语法全场景用例矩阵」穷举；代理内核用真实网络集成测试；性能用基准回归。** 覆盖率目标：workspace ≥ 85%，`rsproxy-rules` ≥ 95%。

### 10.1 规则引擎测试矩阵（重点）

用例组织为**数据驱动**：`tests/corpus/*.toml`，每个 case 声明 `rules 文本 + 请求描述 + 期望命中/期望动作`，一套 corpus 同时驱动单测与 fuzz 种子。维度矩阵：

**A. matcher 维度（每形态 × 正/负例 × 边界）**

| 组 | 覆盖点举例 |
| --- | --- |
| 域名 | 裸域名（不含子域）、`*.` 一级子域、`**.` 任意级子域含自身、带端口、IPv4/IPv6 字面量、大小写、默认端口归一（80/443）、punycode |
| 路径前缀 | `/` 边界（`/path/to` 不匹配 `/path/toxxx`）、尾斜杠、URL 编码字符 |
| 精确 `=` | 带/不带 query（不带则 query 任意）、根路径、`=` 与 glob 混用的解析期报错 |
| glob 通配 | 域名/路径/query 三处 `*` 不跨段、`**` 跨段语义一致性；端口位 `8*`；字面 `*` 的 `\*` 转义；glob 捕获编号顺序（`$1`-`$9`、超过 9 个的报错） |
| 正则 | `i` 标志、锚点、编号/命名捕获（`${name}`）、非捕获组、**backref/lookaround 自动降级 fancy-regex** 的识别用例、执行预算超限按不命中处理的用例 |
| 端口 | `:8080`、`!:8080`、非法端口（0、65536、非数字） |
| 排除 `!` | 排除域名/路径/与正常规则共存时的跳过行为 |
| 恶性输入 | 超长 URL(64KB)、空行/纯注释、非法 UTF-8、嵌套 `${}`、ReDoS 型正则（regex 路径验证线性时间；fancy 路径验证预算熔断） |

**B. action 维度**：每个 v1 动作至少覆盖「基本参数 / `@key` 引用 / `"inline"` 字符串 / `<file>` 文件值 / 模板变量展开 / `$1-$9`、`${name}` 捕获传值 / 非法参数报错」7 类；叠加动作族（⊕：header/replace/append/inject 等）额外覆盖多条叠加顺序；非叠加动作覆盖「首条命中即停」。

**C. when 条件维度**：全部条件（method/host/url/header/res.header/body/status/ip/clientIp/serverIp/chance/env/any）各自的 keyword 与 `/regex/i` 两种子形态；条件内多值「或」；多 `when` 之间「且」；`any()` 显式「或」；`!` 取反；`status`/`res.header` 的响应期延迟评估；`chance` 用注入随机源做确定性测试。

**D. 多级/组合语法（复杂场景）**：

- 同 URL 命中多分组 → 分组顺序优先级；
- `@important` 插队语义、`@disabled` 行级禁用；
- 同动作族多规则文本顺序短路；
- matcher 命中但 `when` 不满足 → 落到下一条同动作族规则（继承 whistle 的关键语义）；
- `skip()` 与其他动作交互（跳过全部 vs 指定动作类）、`bypass` 与 MITM 决策树交互；
- glob 捕获 + 文件值 + 模板变量三者嵌套（`**.a.com/api/** mock(<mock/$1/$2.json>)`）；
- 规则热更新前后请求隔离（旧请求用旧快照）；
- 规范同源校验：`docs/rules-dsl-spec.md` 中每条语法的正反例即 corpus 用例（CI 校验规范与 corpus 一致）；另抽取 whistle 文档（`docs/rules/` 90+ 篇）中的典型场景改写为 rsproxy 规则，验证能力对齐无缺口。

当前 corpus 基线：`rsproxy-rules/tests/corpus/{actions,matchers,conditions,
composition,errors,templates}.toml` 包含 86 个数据驱动 case，公开 API runner 校验动作
族、命中 group/line、响应期条件、explain 和稳定错误字段。`Action::FAMILIES`
定义 46 个公开 family，`actions.toml` 声明同一全集，runner 会比对声明、实现和
实际 resolve 覆盖。37 个 matcher/condition/action/composition case 通过
`<!-- corpus:id -->` 与 `rules-dsl-spec.md` 双向绑定。authority/exact URL、scheme/
port 和空条件参数在发布前严格校验；negated `status`/`res.header` 及嵌套表达式在
响应快照不存在时保持 deferred。`tests/value_matrix.rs` 对 40 个结构化值字段执行
inline、模板/捕获、`@key`、`<file>` 四来源共 160 个解析组合；
`tests/value_sources.rs` 覆盖公开 AST 和 key/file 错误边界；代理层
`value_actions.rs` 验证跨请求、响应、URL、路由、mock、body 和 trace 的真实读取、
模板/编号/命名捕获、UTF-8 与二进制语义。`value_runtime_matrix/` 对同一 40 字段
执行 basic、quoted、`@key`、`<file>`、模板、编号/命名捕获、非法 key 共 280 个
运行时组合。`tests/contracts/whistle_migration.toml` 的独立 runner 将固定 2.10.5 快照中的文档/单测证据
绑定到 46 个支持 action family，并直接解析 Whistle 源码中的 74 个 canonical
protocol 与 22 个显式 alias；每个注册名都必须归类为支持语法/action 或明确的
deferred/removed 能力。`proxy/tests/action_effects/` 通过真实 TCP/TLS
origin、`handle_client` 和客户端观察严格覆盖全部 46 个 family，且拒绝 owner
遗漏或重复。`tests/contracts/whistle_options.toml` 另从本地文档严格分类 56 个
enable、66 个 disable 和 16 类 delete option，并执行所有 implemented 配方；typed
`DeleteOp` 已有双端网络效果，请求 JSON/form 与响应 JSON/JSONP nested path 已由
parser、planner、transform 和真实代理网络测试闭环。option runner 只接受
implemented/native-default/process-config/deferred-v2/removed-v1；process-config 必须
引用真实 CLI 开关，过期的 deferred-m1/m2/m4 状态会失败。§6.3 未列出的压缩、
frame 控制、插件等能力保持明确 v2 边界，不作为 v1 近似 action。

**E. 解析器健壮性**：proptest 随机生成合法/近似合法规则行验证「解析-打印-再解析」不动点；cargo-fuzz 持续 fuzz `parse()` 与 `resolve()`（目标：无 panic、无超线性耗时）。

当前实现：`tests/properties.rs` 以每项 256 个样本验证合法规则经保留源码行输出后
重解析得到相同 AST/统计/请求与响应期 resolution，确定性破坏返回稳定错误字段，
任意 512 字符输入进入所有 parse/resolve/explain API 不 panic。`fuzz/` 提供
nightly ASan/libFuzzer `parse_resolve` target；它与 `tests/fuzz_seeds.rs` 共用
同一个 harness 和 8 个 valid/invalid seed。1000-run smoke、300 秒/463,561 次
workflow 等价运行和 nested body delete 后的 60 秒/121,726 次回归均已通过；
`scripts/verify.sh fuzz` 使用临时 corpus，支持 run-count 或
`RSPROXY_FUZZ_SECONDS` 持续时间限制。`tests/complexity.rs` 对 64KB 大 inline、千行
规则、恶意 delimiter 和 fancy backtrack 输入设置 3 秒单例预算，并对 8x 输入增长
设置宽松比例上限。`.github/workflows/fuzz.yml` 已按日调度 Ubuntu/nightly 的
300 秒运行，失败时上传 crash artifact；远端 workflow 首次执行证据仍待产生。

### 10.2 代理内核集成测试

自建 hyper 桩上游（可编程响应：延迟/分块/断连/畸形头/超大 body/h2/SSE/WS echo），每个用例真实起 proxy 监听随机端口：

- 协议矩阵：h1 明文（含 HTTP/1.0、无 keep-alive、`Expect: 100-continue`）、CONNECT+MITM、CONNECT 透传、h2（客户端 h2 上游 h1、反之）、**请求 trailers（h1/h2 客户端 × h1/h2 origin 四向保真，CL+TE 歧义与禁止 trailer 负例）**、**大请求体流式（Content-Length/chunked、trailer、keep-alive、认证前置、慢上传 deadline、body 规则上下限）**、**gRPC echo（h2 端到端 + trailers 保真）**、WS 握手+双向帧、SSE 流、**上游 mTLS（配置 client-cert 成功 / 未配置明确错误）**、**代理认证（407 → 带凭证成功）**、**大 header 边界（200KB 通过 / 超限 431 带说明、h1 与 h2 双路径）**；
- 规则端到端：host 改写、proxy 链（二级代理桩）、file mock（目录拼接/回退）、statusCode/redirect、req/res 改写全家桶逐个验证线上行为、delay/speed 用时间断言（带容差）；
- 故障注入：上游 RST/超时/半关闭、TLS 握手失败、DNS 失败 → 断言 502/504 与 trace 错误记录；**池饥饿**（慢上游占满 per-key 连接 → 后续请求按 pool_wait 超时报错且 trace 时序可归因）、各阶段超时逐一触发并断言错误标注正确的阶段名；
- MITM：真实 rustls 客户端信任测试根证书完成握手，验证叶子证书 SAN/有效期/缓存命中；
- trace 一致性：并发 1k 请求后，session 条数/时序单调/body 截断标记/drop 计数符合预期；内存预算压力测试（灌 10MB body × N）验证驱逐与截断。

### 10.3 CLI / 控制面测试

- `assert_cmd` + `predicates`：全部子命令的 happy path 与错误路径（daemon 未启动、非法参数、损坏配置）；`--json` 输出 schema 快照（insta）；
- 控制 API contract 测试（直接打 unix socket）；
- daemon 生命周期：start→status→restart(规则保留)→stop、异常 kill 后 pidfile 自愈。

当前控制面在 `rsproxy-control` 内有 34 项 error/server/client/shapes/local-wire 单元测试和 3 项
`tests/public_api.rs` 黑盒 facade 合同；`tests/cli_help.rs` 以真实二进制和 watchdog 验证
分层帮助无副作用；控制路由测试验证 follow 广播、断开及订阅计数归零；Loop 96
补真实 daemon/CLI/curl 运行证据。Loop 97 又以强制 h1/h2 curl 各完成 8MiB
TLS/h2-only origin echo、trailers 和 trace/RSS 验收，并修复 `host(...)` 拨号地址
错误替代 origin TLS SNI/证书身份的问题。HTTP body/framing、上下游 H2 与传输计时的
白盒测试现由 `rsproxy-net` 持有；DNS、typed error、公开 HTTP head 与 deadline 套件
位于该 crate 的集成测试目录，端到端代理/策略交叉由 `rsproxy-engine` 持有。
`scripts/verify.sh matrix` 分别读取 engine/net 两个 package 的 test list，再按声明的 package
逐项执行 34 个 exact owner；CI 必跑该入口。Post-Loop 97 静态验收又
以真实 listener/proxy/client 覆盖 WS server-first/双向帧、mTLS 成功/匿名失败、
h1/h2 200KB/超限 431，以及 IPv6 literal/punycode 路由；并修复 IPv6 URL 重建与
h2 431 诊断窗口。h1→h2 fixture 仍只容忍业务完成后的明确 peer-close I/O，其他
h2 shutdown 错误均失败。新增 16 项 M4 黑盒/配置测试覆盖 completion、JSON/error、
daemon 恢复与产品命令矩阵；Windows-only named-pipe lifecycle case 保留为可选
兼容性测试，不再等待 hosted Windows 证据。

Phase 6 按 D-15 在不扩大公开 API 的前提下，把 net/engine/platform 中 10 个只消费
公开 facade 的套件（35 项测试）移入 crate 级 `tests/`；依赖私有状态机或
test-support 构造器的套件继续留在白盒位置。`rsproxy-platform` 现有 8 项 native
白盒单元测试、10 项公开 CA/error/process 集成测试和 5 项
`tests/public_api.rs` 黑盒 facade 合同。net/control/rules/engine/platform/trace/xtask
七个 `tests/public_api.rs` 目标合计 27 项 facade 合同
（7 + 3 + 2 + 5 + 5 + 3 + 2）；CLI 由可执行文件黑盒目标覆盖。专项命令为
`cargo test -p rsproxy-platform --lib` 与
`cargo test -p rsproxy-platform --test public_api`。

### 10.4 性能与回归

- criterion 微基准：规则解析、单次匹配（按规则规模 100/1k/10k 分档）、证书签发/缓存、trace 事件入队；
- 宏基准脚本（`benches/e2e/`，oha 驱动）：吞吐/延迟场景固化，CI nightly 跑并对比基线；
- 长稳测试：默认 90 分钟持续 1k rps + 混合规则 + trace 开启；至少覆盖 500 万请求和 90 个样本，同时断言 RSS 后半段斜率、内存/FD 与 Trace 生命周期无泄漏。

M0 本地 smoke 仍由 `benches/e2e/benchmark.sh` 提供。M5 另有
`performance.sh`（direct/proxy oha、延迟、RSS）、`whistle.sh`（Whistle pureProxy
精确成功对比）、`benches/criterion/run.sh`（11 项微基准）和 `soak.sh`（默认 90m、
1k QPS、64 并发、混合规则、trace、RSS/FD 与稳态斜率采样）。所有 checker 另有合成 pass/fail
Rust 单测合同，`cargo xtask targets` 以 serde 强类型解析 coverage、criterion、e2e、
soak 与 regression 报告，阈值边界和缺失字段都会失败。
`scripts/verify.sh coverage-report` 使用 llvm-cov 排除测试/
bench/example 后得到 workspace 85.072%、rules 96.263%，均通过目标。

当前仓库仍保留 `.github/workflows/ci.yml` 的三平台 locked check/test/release，
Ubuntu 另执行 fmt、Clippy、结构/质量合同、覆盖率、fuzz target、34-owner 协议矩阵和
46-family action 验收。`performance.yml` 执行每日及 PR Criterion 比较；`fuzz.yml`
执行每日 sanitizer；`release.yml` 对 tag 构建 macOS、Linux、Windows 的 arm64/x64，
Linux 同时覆盖 glibc/musl，并按“原生包 → runtime → 共享 npm/Bun 启动器”的顺序仅发布
到 npm registry。所有 workflow 受 YAML 与文本合同约束。跨平台 target 已完成分包
适配，但本轮只执行当前 macOS ARM64 主机，不把未运行平台描述为已验证。

---

## 11. 项目结构

Cargo workspace（`rsproxy/` 目录）：

```
rsproxy/
├── Cargo.toml                         # workspace 依赖、lint、profile 与统一 package 元数据
├── .cargo/config.toml                 # cargo xtask 别名
├── .github/workflows/
│   ├── ci.yml                         # 三平台 workspace 与 Ubuntu 合同门禁
│   ├── performance.yml                # 同 runner Criterion base/current 回归
│   ├── release.yml                    # 八 target npm 原生包与双启动器发布
│   └── fuzz.yml                       # 每日 Ubuntu/nightly 规则 fuzz
├── crates/
│   ├── rsproxy-net/                   # 无 rsproxy 内部依赖的协议/IO 叶子 crate
│   │   ├── src/lib.rs                 # 显式 HTTP/DNS/IO/deadline/body/pool/H2 facade
│   │   ├── src/http/                  # h1 request head/body/trailer 与 response wire
│   │   ├── src/dns.rs                 # resolver、cache、literal bypass 与统计
│   │   ├── src/async_io.rs            # ReadyIo 到 Tokio AsyncRead/AsyncWrite 适配
│   │   ├── src/{request_deadline,transfer_timing}.rs # 总时限/stage budget 与传输计时
│   │   ├── src/{upstream_body,upstream_pool}.rs # bounded frames 与 keyed admission
│   │   ├── src/upstream_h2/           # message/request body/connection/pool/streaming
│   │   ├── src/downstream_h2/         # message/body/server；泛型 async handler 注入
│   │   ├── src/runtime.rs             # 共享 H2 Tokio runtime
│   │   ├── tests/{dns,errors,http_*_head,request_deadline}.rs # 15 项公开行为测试
│   │   └── tests/public_api.rs        # 只经 facade 的公开 API 合同
│   ├── rsproxy-engine/                # 代理策略与数据面核心域
│   │   ├── src/lib.rs                 # ProxyConfig/SharedState/EngineHandle/RuleStore/serve facade
│   │   ├── src/state.rs               # 数据面配置、运行期装配与共享缓存
│   │   ├── src/handle.rs              # typed status/rules/replay 控制边界
│   │   ├── src/state/                 # MITM failure cache 与白盒测试
│   │   ├── src/rule_store/            # ArcSwap 快照、manifest、watch 与原子文件 IO
│   │   ├── src/proxy.rs               # 私有代理数据面门面与 serve 重导出
│   │   ├── src/proxy/                 # 接入、转发、路由、WS、body、动作、TLS、隧道
│   │   │   ├── server/                # 明文接入、CONNECT、MITM、请求错误
│   │   │   ├── http_flow/             # session、请求体计划、完成/错误归因
│   │   │   ├── h2_bridge/             # net handler 注入、有界请求与增量响应 framing
│   │   │   ├── forward/               # ForwardCtx 与显式上游决策
│   │   │   ├── h1_forward/            # 唯一 h1 路径、连接池与响应 framing
│   │   │   ├── request_stream.rs      # 有 deadline 的上传、trace tee、framing
│   │   │   ├── request_util.rs        # authority/address helper 与共享 throttle pacer
│   │   │   ├── transforms/            # values/delete/content/framing/header 动作
│   │   │   ├── websocket/             # 非阻塞/并发双向转发状态机
│   │   │   ├── connect_tls/           # TLS 握手、观测记录、DNS/TCP/TTFB 时限
│   │   │   ├── tls/                   # TLS 策略、rustls 配置、叶子证书生命周期
│   │   │   └── upstream_response/     # buffered/streaming 响应收尾
│   │   ├── src/proxy/tests/           # 数据面/策略与 34-owner 真实网络交叉测试
│   │   ├── tests/{errors,rule_store}.rs # 10 项公开错误/规则存储行为测试
│   │   ├── tests/public_api.rs        # 状态、规则与 listener facade 公开合同
│   │   ├── benches/certificates.rs    # engine 证书签发/缓存 Criterion 基准
│   │   └── examples/                  # 本地 benchmark origin/client 驱动
│   ├── rsproxy-control/               # 控制协议、传输、认证与稳定输出边界
│   │   ├── src/lib.rs                 # client 与 ControlOptions/State/Listener facade
│   │   ├── src/client.rs              # TCP/Unix/Windows 请求与 NDJSON follow
│   │   ├── src/client/auth.rs         # token 文件、生成、校验与发现
│   │   ├── src/server.rs              # bind/serve 与 ControlState 组装
│   │   ├── src/server/                # auth/query/router/routes/values
│   │   │   └── windows_pipe.rs        # Windows named-pipe listener/stream adapter
│   │   ├── src/shapes/                # JSON/table/HAR 投影
│   │   ├── src/*/tests/               # 34 项 error/server/client/shapes/local-wire 白盒测试
│   │   └── tests/public_api.rs        # 3 项 control facade 黑盒合同
│   ├── rsproxy-platform/              # 无内部依赖的 OS/信任/进程叶子 crate
│   │   ├── src/lib.rs                 # ca/process/system_proxy typed facade
│   │   ├── src/ca.rs                  # 根 CA、存储路径/状态与 trust facade
│   │   ├── src/ca/                    # 根证书/文件存储/macOS-Linux-Windows trust
│   │   ├── src/process.rs             # PID、detach、liveness、terminate、Unix socket path
│   │   ├── src/system_proxy.rs        # typed plan/execute/report facade
│   │   ├── src/system_proxy/          # macOS/Linux/Windows 原生实现与 rollback
│   │   ├── src/**/tests.rs            # 8 项 native trust/system-proxy 白盒测试
│   │   ├── tests/{ca,errors,process}.rs # 10 项公开行为集成测试
│   │   └── tests/public_api.rs        # 5 项 platform facade 黑盒合同
│   ├── rsproxy-cli/
│   │   ├── src/main.rs                # 仅进程退出码与错误输出的薄二进制入口
│   │   ├── src/lib.rs                 # 唯一组合根与可测试 CLI/daemon 库入口
│   │   ├── src/app.rs                 # AppConfig 与 ControlOptions 投影
│   │   ├── src/error.rs               # CliError 聚合、稳定错误码与退出码
│   │   ├── src/logging.rs             # stderr tracing filter、text/JSON 合同
│   │   ├── src/cli/                   # clap/config、命令编排与结果呈现适配器
│   │   │   ├── command.rs             # clap 根命令树 facade，command/ 按命令族分列
│   │   │   ├── ca.rs                  # platform root/trust + engine leaf 的 CLI 适配
│   │   │   ├── daemon.rs              # listener/readiness 编排，process 委托 platform
│   │   │   ├── system_proxy.rs        # typed platform plan/execute 的呈现适配
│   │   │   └── rules/                 # 分组命令、请求元数据与本地 bench 执行
│   │   ├── src/tui/                   # 终端呈现适配器
│   │   ├── src/*/tests/               # CLI/config/presentation 白盒测试
│   │   └── tests/                     # CLI 黑盒集成测试
│   ├── rsproxy-rules/
│   │   ├── src/action/                # Value、HostPool、delete path、replace 与模板校验
│   │   ├── src/parser/                # DSL 核心、动作/条件/delete 编译、底层语法
│   │   ├── src/matcher/               # pattern、动作元数据、条件、URL 模型
│   │   ├── src/{index,matching,planning,resolve}.rs
│   │   ├── src/tests/                 # actions/body_planning/conditions/index/regex
│   │   └── tests/                     # corpus、contracts、固定上游证据、property/fuzz
│   ├── rsproxy-trace/
│   │   ├── src/{lib,model,event}.rs   # Session/TraceEvent/TraceStore 公开门面
│   │   ├── src/store/                 # config/counters/pending/memory/follow/stats/worker
│   │   ├── src/{spill,serialize}.rs   # 磁盘段与 NDJSON 序列化
│   │   ├── src/tests/                 # collector/event/spill 行为测试
│   │   └── tests/                     # trace 公开 API 集成测试
│   └── xtask/                         # 跨平台版本同步与工程自动化入口
│       ├── src/check.rs               # lines/layout/typed-errors/workflows 门禁
│       ├── src/targets.rs             # coverage/performance/soak 强类型报告校验
│       ├── src/release.rs             # release preflight、check 与变更计划
│       ├── src/{check,targets,release}/ # 分域实现与 fixture 单测
│       └── tests/public_api.rs        # release/check/targets typed facade 合同
├── benches/criterion/                 # rules/trace/engine certificate 基准编排与报告
├── benches/e2e/benchmark.sh           # release 代理/curl/Rust client 宏基准
├── benches/e2e/performance.sh         # oha direct/proxy 性能报告
├── benches/e2e/whistle.sh             # 同机 Whistle pureProxy 对比
├── benches/e2e/whistle-driver/        # 固定 2.10.5 的隔离 npm lock
├── benches/soak/soak.sh               # 参数化 90m 高效稳态驱动
├── fuzz/                               # 规则 parse/resolve libFuzzer target 与 seeds
├── packages/npm/                       # 共享 npm/Bun 启动器、runtime、target map 与合同
├── xtask.toml                         # 500 行限制与跨平台扫描排除
├── deny.toml                          # advisory/license/ban/source 供应链策略
├── scripts/verify.sh                  # actions/matrix/bench/stream/package 等进程编排
├── scripts/lib.sh                     # repo root 与稳定 dispatcher 公共函数
└── docs/
    ├── testing.md                     # 测试分层、目录与验证命令
    └── archive/                       # 历史资格证据，不属于活设计面
```

当前组合根依赖方向：`rsproxy-cli → {rsproxy-control, rsproxy-engine,
rsproxy-platform, rsproxy-rules, rsproxy-trace}`；其下为
`rsproxy-control → {rsproxy-engine, rsproxy-rules, rsproxy-trace}`、
`rsproxy-engine → {rsproxy-net, rsproxy-rules, rsproxy-trace}` 与
`rsproxy-trace → rsproxy-rules`。`rsproxy-net`、`rsproxy-platform` 与
`rsproxy-rules` 均不依赖其他 rsproxy crate。`rsproxy-engine` 的 facade 提供
`ProxyConfig`、`SharedState`、`EngineHandle`、`RuleStore` 与 `serve`，且不感知 CLI
参数、控制 API 或呈现格式；engine 的
`proxy/h2_bridge` 通过 `serve_downstream_h2` 的泛型异步 handler 注入业务管道，
`rsproxy-net` 不反向感知 engine 或组合根。control 的 server/client/shapes、Windows
pipe、token auth 与本地 HTTP/1 wire 已完成提取且不依赖 net；platform 的
CA root/storage/trust、system-proxy plan/execute、process/daemon 原语与 Unix control
socket path 也已完成提取。CLI 在启动边界从 platform 读取 root PEM，并以脱敏
`CaMaterial` 注入 `ProxyConfig`；engine 不认识 root 文件名，storage 只承载 leaf
cache。叶证书签发留在 engine 的 `issue_leaf_certificate`，CLI 只负责 clap/config、
组合与呈现。第八个 workspace member `xtask` 提供
`cargo xtask release <VERSION> [--check]`：workspace package version 是唯一版本源，
Cargo/fuzz lock、根分发 manifest、两个 launcher/runtime manifest 统一同步，runtime
的八项 optional dependency 从 `targets.json` 派生；npm 打包通过 `cargo metadata`
读取版本。需要访问私有实现的测试与模块同目录，公开契约测试
放在各 crate 的标准 `tests/` 目录；`cargo xtask check` 约束所有 Rust 文件不超过 500 行、禁止
内联测试模块、禁止测试函数脱离专用测试路径、要求每个 crate 保留公开集成测试目录，
并约束 CI/performance/fuzz/release workflow 的固定清单与必跑命令。

---

## 12. 里程碑规划

当前实现与各里程碑验收线的历史证据已移入 `docs/archive/`。活文档以当前
代码、测试合同和本机验收命令为准。

M0-M5 的“发布”均指当前 Apple M1 Pro / macOS ARM64 本机资格；Linux/Windows
目标 OS 运行和多平台产物不在当前里程碑范围内。

| 里程碑 | 内容 | 验收 |
| --- | --- | --- |
| **M0 骨架**（~2 周） | workspace 搭建、h1 明文代理直通、CONNECT 透传、`run` 前台模式、tracing 日志 | curl 过代理全通，基准脚本可跑 |
| **M1 规则引擎**（~3 周） | **DSL 规范文档（`rules-dsl-spec.md`）**、解析 + 编译 + 匹配全量 matcher/when、`host/upstream/mock/status/redirect/skip` 六动作、corpus 测试框架、`rules check/test` | corpus A/C/D 组通过，10k 规则基准达标 |
| **M2 MITM + 全协议**（~3 周） | CA/叶子证书、TLS MITM、h2、WS、上游 mTLS（client-cert）、代理接入认证、剩余 v1 动作（改写/注入/流控）、values | corpus B 组通过，集成测试矩阵通过，**gRPC 可用（h2 端到端 + trailers 保真）** |
| **M3 Trace**（~2 周） | 采集管道、环形缓冲、资源预算、磁盘 spill、HAR 导出、`trace ls/get/follow/export/stats` | 资源控制压测通过，1GB 大文件内存平稳 |
| **M4 CLI 完备**（~2 周） | daemon 化、控制 API 全量、`replay`、`ca install`、`proxy on/off`、TUI | CLI 测试全绿，手册文档 |
| **M5 打磨发布**（~2 周） | 本机性能专项（§9.3 全指标达标）、fuzz 一轮、长稳、npm/Bun 共享包本机产物、文档 | 历史本机 v0.1.0 发布资格；结构改革后的首次版本为 v0.2.0 |

---

## 13. 结构改革后的遗留项

本轮结构改革完成后仅登记以下两项后续工程问题；它们不阻塞 v0.2.0，也不改变
当前公开合同：

| 遗留项 | 边界与后续条件 |
| --- | --- |
| `rsproxy-engine::proxy` 内部子域化 | `transforms` 与 `forward` 仍共享请求/响应 body、framing 和上下文编排。后续应先用调用图与性能证据识别稳定边界，再考虑内部 facade 或子域重组；不得为了目录整齐扩大公开 API、复制状态，或破坏单一 h1/h2 策略管道 |
| crates.io 发布可行性 | 当前发布合同仅覆盖 npm registry，并由 npm/Bun 客户端验证。是否发布 Rust crates 仍需评估 path dependency 的发布顺序、package metadata/readme/license 完整性、公开 API 的 semver 承诺和 `cargo publish --dry-run`；评估完成前不把 workspace crate 描述为 crates.io 可安装产物 |

`panic = "abort"` 不是遗留项或待优化开关。连接线程依赖 unwind 隔离单连接故障，
release profile 持续保留 unwinding；这是可靠性决策。

---

## 附录 A：与 whistle 能力对照速查

| whistle | rsproxy v1 | 说明 |
| --- | --- | --- |
| 规则 DSL（匹配/动作/条件/值） | ✅ 能力对齐，语法全新 | §6；`rules import --from-whistle` 转换器 v2 |
| 插件体系 / 脚本规则 | ❌ 移除 | 预留 trait，v2 新方案 |
| Web UI | ❌ | 控制 API 已就绪，CLI/TUI 全覆盖 |
| Network 抓包 | ✅ 增强 | 资源预算 + 落盘 + follow |
| HTTPS MITM / H2 / WS | ✅ | rustls + rcgen |
| SOCKS5 接入端口 | 🔶 v2 | 上游 socks:// 转发 v1 支持 |
| Composer 重放 | ✅ `replay` | |
| 系统代理 / CA 安装 | ✅（本机 macOS） | macOS `networksetup` / `security` 为当前验收路径；Windows/Linux 分支、rollback/dry-run/JSON 合同按 best-effort 保留，不要求目标 OS 资格验收 |
| 多实例（-S storage） | ✅ `--storage` | |
| 集群模式 | ❌ 不需要 | 多线程 runtime |
| 代理接入认证（-n/-w） | ✅ `--proxy-auth` | |
| 上游 mTLS（G://clientCert） | ✅ `tls(client-cert=…, client-key=…)` | 直连 HTTPS origin 与 HTTP upstream proxy CONNECT 后的 origin mTLS 已 dogfood |
| TLS 版本/套件（tlsOptions/cipher） | ✅ `tls(min=…, ciphers=…)` | origin-only rustls policy；成功/失败握手均有结构化 trace |

---

## 附录 B：网络请求形式覆盖矩阵

> ✅ v1 支持并有集成测试 · 🔶 计划版本 · ⛔ 明确不做（含理由）

| 请求形式 | 状态 | 说明 |
| --- | --- | --- |
| HTTP/1.0（无 keep-alive） | ✅ | hyper 原生；集成测试覆盖 |
| HTTP/1.1（keep-alive / pipeline 容忍） | ✅ | 下游普通/MITM 连接循环、顺序 pipeline 与上游明文/TLS 多连接池均已 dogfood；upgrade/流式 SSE 正确退出循环 |
| chunked 传输编码 | ✅ | 请求/响应解码与重编码；普通 h1 与客户端 h2 的超限请求均已流式重编码并保留 trailer，h1.1/h2 下游响应均通过有界桥接流式输出 |
| h1 trailers | ✅ | 请求/响应透传 + `res.trailer` 动作可改写 |
| `Expect: 100-continue` | ✅ | 集成测试覆盖 |
| HTTPS（TLS 1.2 / 1.3） | ✅ | 默认 MITM，`bypass` 透传；origin 信任 WebPKI + 原生系统根 + CLI 启动时注入 CA，可用 `tls(min=…, ciphers=…)` 约束 |
| HTTP/2（客户端侧 / 上游侧独立协商，h2↔h1 桥接） | ✅ | h2→h1、h1→h2、h2→h2、请求/响应 trailers、双侧多路复用与 origin h1 ALPN 回退已 dogfood；CONNECT/WS over h2 见后续边界 |
| gRPC（h2 端到端 + trailers 保真） | ✅ | 二进制 unary gRPC frame echo、`application/grpc`、`grpc-status` / `grpc-message` 与规则 trailer 已经 TLS h2→h2 dogfood |
| WebSocket（ws / wss，握手改写 + 帧 trace） | ✅ | 透传与帧解析两档 |
| SSE（text/event-stream） | ✅ | 流式 + 切帧 trace |
| CONNECT 隧道承载任意 TCP | ✅ | 非 TLS 探测，未知协议透传 |
| 上游 mTLS（服务端要求客户端证书） | ✅ | `tls(client-cert=…, client-key=…)`；未配置时上游握手失败并写入 trace error |
| 证书固定（pinning）客户端 | ✅ | 握手失败自动降级透传（TTL 记忆），`--strict-mitm` 关闭降级 |
| 代理接入认证（Proxy-Authorization Basic） | ✅ | `--proxy-auth user:pass`，未认证 407 |
| IPv6 字面量 / punycode 域名 | ✅ | parser corpus 加真实 `::1` origin 与 punycode `host(...)` 路由；URL、Host、拨号地址和 trace identity 均有自动化证据 |
| 大 body（GB 级）流式 | ✅ | release 黑盒测试已通过真实 TCP origin/client 传输 1GiB，端到端与 trace 字节精确一致，4KiB preview 正确，最新运行 RSS 仅增长 3,008KiB；Loop 97 又以真实 h1/h2 curl 各完成 8MiB 到 TLS/h2-only origin 的 echo/trailer/RSS 验收，tunnel/timing/event/follow/spill-snapshot 已由 Loop 96 运行验收 |
| 超大请求/响应头（胖 cookie、JWT 链、网关内部头） | ✅ | 默认 256KB/256 条（whistle 仅 16KB 且需手动调 Node flag）；真实 h1/h2 客户端均验证 200KB 通过、超限返回带说明 431；h2 transport 在应用上限外仅保留固定 64KiB 诊断窗口，上游响应超限写入 `response_head` trace error |
| SOCKS5 接入端口（TCP） | 🔶 v2 | 上游 `upstream(socks://…)` v1 已支持 |
| CONNECT over h2 | 🔶 v2 | 现实流量少，known-gap 标注 |
| WebSocket over h2（RFC 8441） | 🔶 v2+ | 视需求 |
| h2c（明文 h2 upgrade） | 🔶 v2+ | 视需求 |
| HTTP/3 / QUIC | ⛔ | 代理场景客户端自动回落 h2/h1；透传不放行 UDP，无绕过抓包问题 |
| SOCKS5 UDP associate / 任意 UDP 转发 | ⛔ | 超出调试代理定位 |
