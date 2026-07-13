# CI Test Stabilization Plan(CI 测试稳定性修复方案)

Status: proposed. Scope: the five test failures observed in the first full CI
run on GitHub-hosted runners (run
[29239024660](https://github.com/Lakphy/rsproxy/actions/runs/29239024660),
main @ `65f304b`). This document gives exact, line-level guidance because the
fixes cannot be iterated interactively on Linux — they must be right on the
first CI round trip.

> 中文:本方案针对首次全量 CI 运行暴露的 5 个测试失败,给出精确到行的修复
> 指导。因为无法在 Linux 上实时调试,修复必须一次改对,靠 CI 往返验证。

## Failure inventory(失败清单)

| Test | ubuntu | macos runner | Local macOS (dev) | Error |
| --- | --- | --- | --- | --- |
| `rsproxy-engine proxy::tests::h2_downstream_streaming::downstream_h2_request_and_response_stream_with_bounded_backpressure` | FAIL | FAIL | pass | `hyper::Error(Io, NotConnected)` in proxy server thread |
| `rsproxy-engine proxy::tests::origin_h2_streaming::oversized_h2_upload_streams_to_h2_origin_with_trailers` | FAIL | FAIL | pass | same |
| `rsproxy-engine proxy::tests::protocol_matrix::headers::h2_large_header_accepts_200kb_and_rejects_over_limit_with_431` | FAIL | FAIL | pass | same |
| `rsproxy-net downstream_h2::tests::downstream_h2_server_delegates_streams_through_callback` | FAIL | pass | pass | same, in `serve_downstream_h2` unwrap |
| `rsproxy-net upstream_h2::tests::timeouts::ttfb_and_request_total_timeouts_have_independent_scopes` | FAIL | pass | pass | `assertion failed: slow_body.ttfb_ms < 40` |

The same failures reproduced independently in three jobs (workspace/nextest,
distribution/`cargo test`, coverage/instrumented `cargo test`), so they are
environmental, not runner flakes of a single execution. The common variable is
slow, low-core shared runners — not the operating system (the macOS runner
fails where the faster local macOS passes).

> 中文:同一批失败在三个独立 job 里都复现,说明是环境性问题而非偶发抖动。
> 共同变量是共享 runner 的低核数与调度延迟,不是操作系统(macOS runner 也
> 挂,而更快的本地 mac 通过)。

## Root cause A — client `abort()` races the h2 GOAWAY (4 tests)(根因 A:abort 与 GOAWAY 竞态)

All four `NotConnected` failures share one teardown anti-pattern. The hyper
HTTP/2 client driver is spawned as a task, and the test ends it with:

```rust
connection.abort();
let _ = connection.await;
```

`abort()` kills the driver mid-flight: no GOAWAY frame, the TCP socket drops
abruptly, possibly mid-frame. The server side of the test — the real proxy in
`spawn_proxy` (engine) or `serve_downstream_h2` (net) — sees the abrupt close
as an I/O error and the test harness `unwrap`s it:

- `crates/rsproxy-engine/src/proxy/tests/support.rs:53` —
  `handle_client(stream, state.clone()).unwrap()` panics in the server
  thread; the test's `proxy_server.join().unwrap()` then fails with
  `Err(Any)`.
- `crates/rsproxy-net/src/downstream_h2/tests/mod.rs:176` — the serve call's
  `.unwrap()` panics; `server.join().unwrap()` at line 290 fails.

On a fast machine the server usually finishes its read loop before the abort
lands, so the race stays hidden locally. On a loaded 2–3-core runner the
abort regularly wins. The proof that abort is the culprit:
`protocol_matrix/headers.rs:63` already does `drop(sender)` (which is all a
graceful shutdown needs) and then immediately aborts anyway — and still
fails.

The fix is to make teardown **deterministic by construction** instead of
timing-dependent: drop every `SendRequest` handle, then *await* the driver,
which sends GOAWAY, completes the TLS/TCP shutdown, and lets the server
return `Ok`. No sleeps, no tuned windows — pure ordering.

> 中文:四个 NotConnected 失败都是同一个收尾反模式——`abort()` 掐断客户端
> h2 驱动任务,没有 GOAWAY,TCP 被粗暴关闭;服务端(被测代理本身)把突然
> 断连当 I/O 错误,测试脚手架对它 `unwrap` 导致 panic。本地机器快所以竞态
> 藏住了。修复思路:改为"drop 所有 SendRequest 句柄 → await 驱动任务自然
> 结束"——这是**构造上确定**的顺序,与时序无关。

### Exact edits(精确修改)

Apply the same two-part pattern at each site. Part 1 makes the driver's
result observable; part 2 replaces the abort with a bounded graceful await.

**Part 1 — spawn the driver directly** (its output is
`Result<(), hyper::Error>`; awaiting the `JoinHandle` then proves the
connection closed cleanly):

```rust
// before
let connection = tokio::spawn(async move {
    let _ = connection.await;
});
// after
let connection = tokio::spawn(connection);
```

**Part 2 — graceful teardown** (replaces `connection.abort();` +
`let _ = connection.await;`):

```rust
tokio::time::timeout(Duration::from_secs(3), connection)
    .await
    .expect("h2 client connection should close within the shutdown deadline")
    .expect("h2 client connection task should not panic")
    .expect("h2 client connection should shut down cleanly after GOAWAY");
```

The 3-second timeout matches the file-local idiom for bounded awaits and
turns a would-be hang into a diagnosable failure.

Per-site specifics (verify the surrounding lines before editing; line
numbers are from `65f304b`):

1. `crates/rsproxy-engine/src/proxy/tests/h2_downstream_streaming.rs`
   - Part 1 at lines 138–140 (`connection_task`).
   - Part 2 at lines 273–274 (`connection_task.abort(); let _ = ...`).
   - No `drop(sender)` needed: `sender` was moved into the request task at
     line 218 and that task completed by line 239, so no live `SendRequest`
     remains — the driver can finish as soon as it is awaited.
2. `crates/rsproxy-engine/src/proxy/tests/origin_h2_streaming.rs`
   - Part 1 at lines 268–270.
   - Part 2 at lines 320–321.
   - `sender` was moved into the spawn at line 278 and its response awaited
     at lines 309–313; the response body is fully collected at line 317. No
     extra `drop` needed.
3. `crates/rsproxy-engine/src/proxy/tests/protocol_matrix/headers.rs`
   - Part 1 at lines 51–53.
   - Keep `drop(sender);` at line 63; Part 2 replaces lines 64–65.
4. `crates/rsproxy-net/src/downstream_h2/tests/mod.rs`
   - Part 1 at lines 191–193.
   - Keep `drop(sender);` at line 276; Part 2 replaces lines 277–278.

Do **not** loosen the server-side `unwrap`s (`support.rs:53`,
`downstream_h2/tests/mod.rs:176`): with a graceful client they must return
`Ok`, and keeping them strict preserves the assertion that the product
handles a well-behaved close cleanly. A tolerance fallback exists (below)
but is a second stage, not part of this change.

> 中文:四处统一两步改法——①驱动任务直接 `tokio::spawn(connection)`,让
> 结果可断言;②用带 3 秒超时的 await 替换 abort。站点 1、2 的 sender 已
> 随请求任务结束而释放,无需额外 drop;站点 3、4 保留已有的
> `drop(sender)`。服务端的 unwrap **保持严格**:优雅关闭后必须是 Ok,这
> 本身就是断言。

## Root cause B — hard-coded 40 ms bound (1 test)(根因 B:硬编码 40ms 断言)

`crates/rsproxy-net/src/upstream_h2/tests/timeouts.rs:117` asserts
`slow_body.ttfb_ms < 40`. The semantic under test is "TTFB stops at the
response head and excludes the 80 ms body delay" — but 40 ms of headroom is
pure scheduler noise on a shared runner. The two *timeout-error* assertions
in the same test (lines 102, 142–145) are direction-safe (a slower machine
still times out) and stay untouched.

Fix: widen the delay so the invariant stays sharp while the noise margin
grows ~6×:

- Line 64: `Duration::from_millis(80)` → `Duration::from_millis(250)`
  (the `/slow-body` `DelayedBody` delay only; keep the `/slow-head` 80 ms
  sleep at line 60 — it is on the direction-safe path).
- Line 117: `assert!(slow_body.ttfb_ms < 40);` →
  `assert!(slow_body.ttfb_ms < 250);` — still proves TTFB excluded the body
  delay (had it been included, `ttfb_ms >= 250`).
- Line 126: `assert!(response_body.receive_ms().unwrap() >= 60);` →
  `>= 200` (same 20% margin below the delay as before).

Cost: one extra ~170 ms wait in the `slow_body` leg; the `total_response`
leg still errors at its 40 ms deadline regardless of the larger delay.

> 中文:`ttfb < 40ms` 的 40ms 余量在共享 runner 上就是调度噪声。把
> `/slow-body` 的正文延迟从 80ms 提到 250ms,断言改为 `ttfb < 250` 且
> `receive_ms >= 200`——被测不变量(TTFB 不含正文延迟)依然锋利,噪声容忍
> 度扩大 6 倍;两个方向安全的超时断言不动。

## Validation without a Linux box(无 Linux 环境的验证协议)

1. **Determinism first**: both fixes remove timing dependence rather than
   tune it — Class A is a pure ordering change; Class B keeps a 6× margin on
   an invariant that is boolean in nature. Review against that standard.
2. **Local full run** (must stay green):
   `cargo nextest run --workspace --all-targets --no-fail-fast --locked`
3. **Local stress of the five tests under CPU load** (widens the race window
   the runners exposed; run from the repo root):

   ```sh
   for i in 1 2 3 4 5 6 7 8; do yes > /dev/null & done
   for i in $(seq 1 30); do
     cargo nextest run --locked \
       -E 'test(downstream_h2_request_and_response_stream_with_bounded_backpressure) + test(oversized_h2_upload_streams_to_h2_origin_with_trailers) + test(h2_large_header_accepts_200kb_and_rejects_over_limit_with_431) + test(downstream_h2_server_delegates_streams_through_callback) + test(ttfb_and_request_total_timeouts_have_independent_scopes)' \
       || { echo "FAILED at iteration $i"; break; }
   done
   pkill -f "^yes$"
   ```

   Before the fix this loop should reproduce Class A within a few dozen
   iterations; after the fix it must survive all 30.
4. **CI round trip**: branch → PR → approve the `ci-approval` gate → all
   three workspace legs plus distribution and coverage must be green. Then
   `gh run rerun <run-id>` twice more; three consecutive green runs on the
   same commit is the acceptance bar (runs are free on this public repo).
5. **Soak**: the nightly performance/fuzz schedules and subsequent PR runs
   serve as ongoing regression detection; treat any recurrence as a new
   finding, not a retry candidate.

> 中文:验证顺序——先确认修复是"去时序化"而非"调参";本地全量 nextest;
> 再在 8 个 `yes` 进程制造的 CPU 压力下循环 30 遍跑这 5 个测试(修复前应
> 能复现,修复后必须全绿);最后走 CI:开 PR、批准审批门、三平台全绿后同
> 一 commit 再 rerun 两次,连续三绿为验收线。

## Explicit non-goals(明确不做的事)

- **No `nextest --retries` / test quarantine** — retries mask exactly the
  class of ordering bugs this plan fixes.
- **No server-side error allowlist yet** — if (and only if) a post-fix CI
  run still shows a disconnect-class error in `support.rs:53` or
  `downstream_h2/tests/mod.rs:176`, add a stage-2 tolerance that accepts
  only `NotConnected` / `ConnectionReset` / `BrokenPipe` / `UnexpectedEof`
  by message match and panics on anything else — and record which test
  triggered it before doing so.
- **No CI-only `cfg` or environment-detection branches in tests** — the same
  assertions must hold everywhere.

> 中文:不引入重试或隔离机制(会掩盖本类 bug);暂不放宽服务端 unwrap,
> 只有修复后 CI 仍出现断连类错误时才加白名单容忍(且要先记录触发者);
> 不允许测试里出现"CI 环境特判"。

## Open item(待观察)

The Windows workspace leg had not finished when this plan was written. If it
reports additional failures, triage them against the two root-cause classes
above before inventing a third; loopback networking and timer granularity on
Windows usually surface the same two shapes.

> 中文:Windows leg 结果未出;若有新增失败,先按上述两类根因归类,不要
> 急于引入第三种解释。
