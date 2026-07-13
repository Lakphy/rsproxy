# Development and Release Process(开发与发布流程)

Standard operating procedure for day-to-day development and for shipping a
release. English is normative; each section ends with a Chinese summary.

> 中文:本文档是日常开发与版本发布的标准操作流程(SOP)。英文内容为准,
> 每节末尾附中文摘要。

## Overview(概览)

rsproxy ships through two channels, both driven by pushing a `v*` git tag:

1. **npm registry** — ten `@rsproxy/*` packages: eight native platform
   packages (`@rsproxy/darwin-arm64`, `@rsproxy/linux-x64-gnu`, …) plus the
   `@rsproxy/runtime` resolver and shared `@rsproxy/cli` launcher, all published
   with npm provenance.
2. **GitHub Releases** — one `tar.gz` (or `zip` on Windows) archive per Rust
   target containing the `rsproxy` binary, `LICENSE`, and `README.md`, plus a
   `SHA256SUMS` manifest. Release notes are extracted from `CHANGELOG.md`.

Cargo crates are **not** published to crates.io (`publish = false` across the
workspace). Versioning follows [Semantic Versioning](https://semver.org/); the
single source of truth is `version` in the root `Cargo.toml`, and
`cargo xtask release` keeps `package.json` and every npm package manifest in
sync with it. `CHANGELOG.md` follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

> 中文:发布有两个渠道,均由推送 `v*` 标签触发——npm(10 个 `@rsproxy/*`
> 包,带 provenance)和 GitHub Release(每个平台一个二进制归档 +
> SHA256SUMS + 基于 CHANGELOG 的发布说明)。不发布 crates.io。版本号以根
> `Cargo.toml` 为唯一事实来源,由 `cargo xtask release` 同步到所有清单。

## Branch and PR workflow(分支与合并流程)

- `main` is the only long-lived branch and must always be releasable.
- Pull requests are the default path for changes. During early development,
  repository admins may push directly to `main` (`enforce_admins` is off);
  the PR gate below still applies to everyone else and to any PR.
- Branch names: `feat/<topic>`, `fix/<topic>`, `docs/<topic>`,
  `chore/<topic>`, or `hotfix/<version>` (see the hotfix section).
- Commit messages follow the conventional-commit style already used in the
  history: `feat:`, `fix:`, `refactor:`, `docs:`, `chore:`, `test:`, `perf:`.
- **CI on PRs is approval-gated**: every push to a PR creates a CI run that
  waits (at no runner cost) on the `ci-approval` environment. Start it from
  the PR page via "Review deployments" → approve `ci-approval`, or from the
  run page. Branch protection on `main` requires the approval gate and every
  CI job to succeed before merging, so an unapproved or rejected run blocks
  the merge rather than bypassing it.
- Pushes to `main` and merge-queue runs execute immediately without approval.
  The CI workflow listens to `merge_group`, so a GitHub merge queue can be
  enabled at any time without workflow changes.
- User-visible changes add a bullet under `## [Unreleased]` in `CHANGELOG.md`
  in the same PR.

> 中文:只有 `main` 一条长期分支,所有改动走 PR,commit 用约定式前缀
> (`feat:`/`fix:` 等)。PR 上的 CI 不会自动跑:每次推送会挂起一个等待审批
> 的运行(等待期间不消耗资源),在 PR 页面点 "Review deployments" 批准
> `ci-approval` 后开始;main 的分支保护要求审批门和所有 CI job 全绿才能
> 合并。用户可见的改动需在同一 PR 中更新 CHANGELOG 的 `Unreleased` 段。

## Quality gates(质量门禁)

`ci.yml` runs on every push to `main`, and on PRs once the manual approval
gate is passed (pushes to other branches do not trigger CI; the PR run covers
them). Superseded PR runs are cancelled automatically. The jobs and their
local equivalents:

| CI job | What it enforces | Local equivalent |
| --- | --- | --- |
| Workspace (3 OS) | portable contracts, `cargo check/test/build` on Linux/macOS/Windows | `cargo xtask check workflows` (plus `lines`, `layout`, `typed-errors`), `cargo nextest run --workspace --all-targets --no-fail-fast --locked` |
| Minimum Rust 1.88 | workspace compiles at the MSRV | `rustup toolchain install 1.88.0 && cargo +1.88.0 check --workspace --locked` |
| Formatting, Clippy, and docs | rustfmt, `clippy -D warnings`, rustdoc `-D warnings` | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` |
| Repository contracts | `cargo xtask check all` (incl. public-API snapshots), shell syntax, fuzz target compiles | `cargo xtask check all` |
| Distribution contracts | protocol matrix, CLI action effects, npm package contract | `./scripts/verify.sh matrix`, `./scripts/verify.sh actions`, `./scripts/verify.sh package` |
| Supply chain policy | cargo-deny advisories, bans, licenses, sources | `cargo deny check --all-features --locked` |
| Production line coverage | workspace line coverage ≥ 85%, `rsproxy-rules` ≥ 95% | `./scripts/verify.sh coverage-report` |

Two workflows watch quality outside the PR loop; both run on a daily schedule
and can also be dispatched manually. They are **advisory, not release
gates**: a red run opens an issue to investigate a regression or crash, but
never blocks tagging or shipping.

- `performance.yml` (daily 02:41 UTC): Criterion benchmarks against absolute
  targets, and a 10% regression gate against the parent commit.
- `fuzz.yml` (daily 03:17 UTC): replays versioned fuzz seeds, then runs a
  five-minute libFuzzer campaign against the rules parser; crashes upload as
  artifacts. Note: GitHub disables scheduled workflows after 60 days without
  repository activity and emails a re-enable prompt.

> 中文:CI 每个 PR 只跑一次(push 仅 main 触发,过期 PR 运行自动取消)。
> 七个 job 分别覆盖三平台构建测试、MSRV、格式/Clippy/文档、仓库契约、
> 分发契约、供应链策略、行覆盖率(workspace ≥85%,rules ≥95%)。
> 性能基准和模糊测试每日定时运行(也可手动触发),定位是**质量雷达而非
> 发布闸门**:变红开 issue 跟进,不阻断发布。
> 注意:仓库 60 天无活动时 GitHub 会自动停用定时任务,需手动重新启用。

## Public API snapshots(公共 API 快照)

Each crate's public surface is snapshotted in `crates/<crate>/api.txt` and
gated by `cargo xtask check api`. Prerequisites (matching CI exactly):

```sh
rustup toolchain install nightly-2026-07-10 --profile minimal
cargo install cargo-public-api --version 0.52.0 --locked
```

When a PR intentionally changes a public API:

1. Run `cargo xtask check api` — it prints the first drifted line.
2. Run `cargo xtask check api --bless` to regenerate the snapshots.
3. Commit the `api.txt` diff in the same PR; reviewers treat it as the
   API-change review surface.

Never bless snapshots to silence a drift you did not intend — that is the
gate catching an accidental breaking change.

> 中文:公共 API 快照存于各 crate 的 `api.txt`,有意的 API 变更需运行
> `cargo xtask check api --bless` 并将 diff 随 PR 提交评审;
> 非预期的漂移说明引入了意外破坏性变更,不要用 bless 压掉。

## Workflow contracts(工作流契约)

`.github/workflows/` is itself under contract:
`crates/xtask/src/check/workflow_contracts.rs` pins the exact workflow
inventory, required command strings, pinned action versions, and forbidden
patterns (floating action tags, `continue-on-error`, workflow-level
`contents: write`, …). Consequences:

- Any edit to a workflow file must update `workflow_contracts.rs` **in the
  same commit**, or `cargo xtask check workflows` fails on every platform.
- Dependabot PRs that bump a pinned action (e.g. `actions/checkout@v6` →
  `@v7`) stay red until the matching pin in `workflow_contracts.rs` is
  updated in that PR. This lockstep is intentional.
- Verify locally with `cargo xtask check workflows` and `cargo test -p xtask`.

> 中文:workflow 文件本身受契约约束,任何改动必须在同一 commit 中同步更新
> `workflow_contracts.rs`,否则 CI 必挂;Dependabot 升级 action 的 PR
> 同样需要补契约中的版本钉。

## Preparing a release(发布准备)

1. Decide the new version `X.Y.Z` per SemVer (pre-1.0: breaking changes bump
   the minor version).
2. In `CHANGELOG.md`, rename `## [Unreleased]` content into a new
   `## [X.Y.Z] - YYYY-MM-DD` section (keep an empty `Unreleased` above it).
   The release automation extracts exactly this section as the GitHub release
   notes and fails if it is missing.
3. Synchronize every manifest:

   ```sh
   cargo xtask release X.Y.Z          # writes Cargo.toml, package.json, npm manifests
   cargo xtask release X.Y.Z --check  # verifies the result
   ```

4. Open a release PR (`chore: release vX.Y.Z`) with the version bump and
   changelog; merge it once CI is green. CI on the release commit is the
   only pipeline gate — the scheduled performance/fuzz workflows are
   advisory and do not block tagging.

> 中文:确定版本号 → 把 CHANGELOG 的 Unreleased 整理成 `## [X.Y.Z] - 日期`
> 段(发布说明即取自该段,缺失会导致发布失败)→ `cargo xtask release X.Y.Z`
> 同步所有清单并用 `--check` 复核 → 提发布 PR,CI 绿后合并。发布唯一的
> 流水线门禁是发布 commit 上的 CI;performance/fuzz 仅供参考,不阻断打标。

## Tagging and automation(打标签与自动化发布)

Tag the merged release commit on `main` and push the tag:

```sh
git switch main && git pull
git tag vX.Y.Z
git push origin vX.Y.Z
```

Pushing the tag runs `release.yml` (a concurrency group serializes runs; an
in-flight release is never cancelled):

1. **native** (8-target matrix: Linux gnu/musl × x64/arm64, macOS x64/arm64,
   Windows x64/arm64) — verifies the tag matches every manifest version
   (`cargo xtask release "$version" --check`), builds the `rsproxy` binary,
   packages the npm native `.tgz`, and packages the GitHub archive
   (`rsproxy-vX.Y.Z-<rust-triple>.tar.gz` / `.zip`).
2. **publish** — downloads all native packages, builds the runtime and shared
   launcher packages, verifies the 10-package inventory, and runs
   `npm publish --provenance` for every package.
3. **github-release** — runs only after npm publishing succeeds; downloads
   the 8 archives, writes `SHA256SUMS`, extracts the `X.Y.Z` section from
   `CHANGELOG.md`, and creates the GitHub release with `gh release create
   --verify-tag`. This job is the only place granted `contents: write`.

A `workflow_dispatch` run of `release.yml` from a branch exercises the native
build matrix **and the npm packaging step** as a dry run; the version check,
archive, publish, and release steps are all tag-gated and skipped. Run this
drill before tagging whenever `release.yml`, `scripts/package-npm.sh`, or
`packages/npm/scripts/` changed — the packaging step runs on all three
operating systems only inside this workflow, so the drill is the first place
platform-specific packaging bugs can surface.

> 中文:在 main 上打 `vX.Y.Z` 标签并推送即触发发布:8 平台构建 → npm 发布
> (10 包,带 provenance)→ npm 成功后才创建 GitHub Release(归档 +
> SHA256SUMS + CHANGELOG 提取的说明)。手动 `workflow_dispatch` 演练构建
> 矩阵和 npm 打包步骤但不发布;凡是改过 release.yml 或打包脚本,打 tag 前
> **必须**先空跑一次演练——打包步骤只有这条 workflow 会在三个操作系统上
> 执行,平台特有的打包问题只能在演练里提前暴露。

## Verifying a release(发布验证)

```sh
npm view @rsproxy/cli version          # expect X.Y.Z
gh release view vX.Y.Z                 # 8 archives + SHA256SUMS attached
npx @rsproxy/cli@X.Y.Z --version       # launcher smoke test
bunx --bun @rsproxy/cli@X.Y.Z --version # same artifact under Bun
```

To verify an archive: download it and `SHA256SUMS`, then
`shasum -a 256 --check SHA256SUMS --ignore-missing`.

If the `github-release` job failed after npm publishing succeeded, fix the
cause and re-run just that job from the workflow run page — the npm packages
are immutable and must not be republished.

> 中文:发布后用 `npm view`、`gh release view` 和 `npx` 冒烟验证;归档用
> `shasum -a 256 --check` 校验。若 npm 已发成功而 GitHub Release 失败,
> 只重跑该 job,切勿重发 npm 包。

## Hotfix procedure(热修复流程)

For a critical fix against the latest release `vX.Y.Z` when `main` has
already diverged:

1. `git switch -c hotfix/X.Y.(Z+1) vX.Y.Z`
2. Apply (or cherry-pick) the fix; add the changelog section; run
   `cargo xtask release X.Y.(Z+1)`.
3. Open a PR from the hotfix branch so CI validates it, then tag the approved
   hotfix commit: `git tag vX.Y.(Z+1) && git push origin vX.Y.(Z+1)`. The tag
   pipeline is identical to a normal release.
4. Merge or cherry-pick the fix back to `main` so the next release includes
   it, and reconcile the changelog.

If `main` has not diverged from the release, skip the branch and release from
`main` as usual with a patch bump.

> 中文:若 main 已领先于线上版本,从 tag 拉 `hotfix/` 分支,修复 + 版本
> patch 递增 + CHANGELOG,经 CI 验证后直接在该分支打 tag 发布,事后把修复
> 合回 main;若 main 未领先,直接按正常流程发 patch 版本。

## Prerequisites and secrets(前置条件与机密)

- **`NPM_TOKEN`** (repository secret) — an npm granular automation token with
  publish rights on the `@rsproxy` scope. Provenance additionally requires
  the workflow's `id-token: write` permission and a public repository, both
  already configured. Rotate the token on expiry; a future option is npm
  [Trusted Publishing](https://docs.npmjs.com/generating-provenance-statements)
  (OIDC), which removes the long-lived token entirely.
- **`GITHUB_TOKEN`** — the default workflow token; sufficient for
  `gh release create`. No personal access token is needed.
- **Dependabot** (`.github/dependabot.yml`) — weekly grouped cargo updates,
  monthly npm and github-actions updates. Every Dependabot PR runs the full
  CI gate (after manual approval, like any PR); cargo updates are additionally
  screened by cargo-deny. The `benches/e2e/whistle-driver` fixture dependency
  is intentionally excluded.
- **One-time repository configuration** (already applied; recorded here for
  reproducibility):
  - Environment `ci-approval` with the repository owner as a required
    reviewer — this is what holds PR CI runs until approval.
  - Branch protection on `main` (force-pushes and deletions blocked;
    `enforce_admins` intentionally off during early development so admins
    can push directly) requiring these checks: `Manual CI approval`, the
    three `Workspace (...)` legs, `Minimum Rust 1.88`,
    `Formatting, Clippy, and docs`, `Repository contracts`,
    `Distribution contracts`, `Supply chain policy`, and
    `Production line coverage`. The gate itself must stay in the required
    list: GitHub treats skipped checks as passing, so without it a rejected
    gate (all jobs skipped) would incorrectly allow the merge.

> 中文:发布只需两个凭据——`NPM_TOKEN`(@rsproxy scope 的自动化 token,
> 到期轮换,未来可迁移到 OIDC Trusted Publishing)和默认的 `GITHUB_TOKEN`。
> Dependabot 每周(cargo)/每月(npm、actions)自动开升级 PR,一律过全量
> CI 与 cargo-deny;whistle 基准夹具依赖除外。
