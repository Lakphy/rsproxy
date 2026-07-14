# Development and Release Process

This document describes the repository's contribution and release contracts.
The workflow files are authoritative for automation; commands here are the
human operating procedure around them.

## Development workflow

`main` is the long-lived, releasable branch. Use a focused branch and pull
request for normal changes. Suggested prefixes are `feat/`, `fix/`, `docs/`,
`chore/`, and `hotfix/`.

Commit messages use the conventional prefixes already present in project
history: `feat:`, `fix:`, `refactor:`, `docs:`, `chore:`, `test:`, and `perf:`.
Add a bullet under `CHANGELOG.md`'s `Unreleased` section for user-visible
changes.

Pull-request CI waits for approval through the `ci-approval` GitHub environment;
`main`, merge-queue, and manually dispatched runs start immediately. Required
branch-protection checks and environment reviewers are repository settings, so
administrators must keep those settings aligned with `.github/workflows/ci.yml`.

Run the standard local checks from [Testing](testing.md) before requesting
review.

## Distribution model

A version tag publishes two artifact sets:

1. npm registry: eight native `@rsproxy/*` packages, `@rsproxy/runtime`, and
   the shared `@rsproxy/cli` launcher, all with npm provenance.
2. GitHub Releases: eight native archives plus `SHA256SUMS`, with release notes
   extracted from `CHANGELOG.md`.

Workspace crates set `publish = false` and are not released to crates.io.

The root `Cargo.toml` workspace version is authoritative. `cargo xtask release`
synchronizes the root JavaScript manifest and npm package manifests. Package
building reads Cargo metadata and rejects version drift.

## CI quality gates

The pull-request/main CI jobs are:

| Job | Contract |
| --- | --- |
| Workspace (Linux/macOS/Windows) | structural checks, locked check/tests, and release builds |
| Minimum Rust 1.88 | MSRV compile check |
| Formatting, Clippy, and docs | rustfmt, denied Clippy warnings, and rustdoc warnings |
| Repository contracts | `cargo xtask check all`, shell syntax, and fuzz-target compile |
| Distribution contracts | protocol matrix, action effects, and npm/Bun package contract |
| Supply chain policy | cargo-deny advisories, bans, licenses, and sources |
| Production line coverage | workspace coverage at least 85% and rules coverage at least 95% |

Scheduled performance and fuzz workflows are regression monitors, not tag
prerequisites enforced by `release.yml`. Investigate red scheduled runs before
shipping when they are relevant to the release.

## Public API snapshots

Each library crate has an `api.txt` snapshot. Reproduce the CI checker with:

```sh
rustup toolchain install nightly-2026-07-10 --profile minimal
cargo install cargo-public-api --version 0.52.0 --locked
cargo xtask check api
```

For an intentional public API change:

```sh
cargo xtask check api --bless
git diff -- crates/*/api.txt
```

Commit and review the snapshot diff with the implementation. Do not bless an
unexplained drift.

## Workflow contracts

`cargo xtask check workflows` pins workflow inventory, triggers, permissions,
action versions, required commands, and forbidden patterns. An intentional
workflow edit normally requires a matching update to
`crates/xtask/src/check/workflow_contracts.rs` in the same commit.

This includes dependency-bot action upgrades: a pinned action change remains
red until the executable workflow contract is updated and reviewed.

## Prepare a release

Choose `X.Y.Z` according to Semantic Versioning. Before 1.0, use a minor bump
for a breaking change.

1. Keep an empty `## [Unreleased]` section in `CHANGELOG.md` and move its
   completed entries into `## [X.Y.Z] - YYYY-MM-DD`. The release workflow
   requires that exact heading.
2. Synchronize and verify every manifest:

   ```sh
   cargo xtask release X.Y.Z
   cargo xtask release X.Y.Z --check
   ```

3. Run the local release-relevant gates:

   ```sh
   cargo xtask check all
   cargo test --workspace --all-targets --no-fail-fast --locked
   ./scripts/verify.sh actions
   ./scripts/verify.sh matrix
   ./scripts/verify.sh package
   ```

4. Open and merge a release PR such as `chore: release vX.Y.Z` after CI passes.

If `release.yml`, `scripts/package-npm.sh`, or `packages/npm/scripts/` changed,
manually dispatch `release.yml` before tagging. A branch dispatch builds every
native target and packages npm artifacts but skips tag-only version checks,
release archives, publishing, and GitHub Release creation.

## Tag and publish

Tag the reviewed release commit:

```sh
git switch main
git pull --ff-only
git tag vX.Y.Z
git push origin vX.Y.Z
```

`release.yml` then runs in this order:

1. The eight-target native matrix verifies the tag/version match, builds and
   executes each binary, creates a native npm package, and creates a GitHub
   archive.
2. The publish job downloads the native packages, builds runtime and launcher
   packages, verifies the ten-package inventory, and publishes native packages
   first, runtime next, and CLI last with provenance.
3. After npm succeeds, the GitHub Release job verifies eight archives, writes
   `SHA256SUMS`, extracts release notes, and creates the release. This is the
   only job with `contents: write`.

The release concurrency group never cancels an in-flight run.

## Verify a release

After registry propagation:

```sh
npm view @rsproxy/cli version
gh release view vX.Y.Z
npx @rsproxy/cli@X.Y.Z --version
bunx --bun @rsproxy/cli@X.Y.Z --version
```

Confirm the GitHub Release contains eight platform archives and
`SHA256SUMS`. To verify a downloaded archive on a platform with `shasum`:

```sh
shasum -a 256 --check SHA256SUMS --ignore-missing
```

New npm packages can briefly return 404 from registry read paths after a
successful publish. Use the publish job's npm confirmation lines as the first
source of truth, then retry registry queries before diagnosing a failure.

## Recover a failed release

First determine whether any package at `X.Y.Z` is public.

| Failure point | Safe recovery |
| --- | --- |
| Native job or publish job before its first successful `npm publish` | Fix `main`, pass CI, delete and recreate the tag at the fixed commit |
| Publish job after one or more packages are public | Do not move the tag or republish an existing package version; complete manually only with a reviewed plan, otherwise prepare a patch release |
| GitHub Release job after npm completed | Rerun the failed job; do not retag or republish |

Move a tag only when no artifact at that version is public:

```sh
git push origin :refs/tags/vX.Y.Z
git tag -d vX.Y.Z
git tag vX.Y.Z <fixed-commit>
git push origin vX.Y.Z
```

Once any npm package at a version is public, that version and tag are
immutable.

## Hotfixes

If `main` has diverged from the latest release, branch from the release tag,
apply the fix, add a changelog entry, and use the next patch version:

```sh
git switch -c hotfix/X.Y.N vX.Y.Z
cargo xtask release X.Y.N
cargo xtask release X.Y.N --check
```

Validate the hotfix through a pull request, tag the approved commit, then merge
or cherry-pick the fix back to `main`. If `main` has not diverged, use the normal
release flow.

## Required credentials and repository settings

- `NPM_TOKEN`: npm granular automation token with publish rights for the
  `@rsproxy` scope. The publish job also has `id-token: write` for provenance.
- `GITHUB_TOKEN`: the default workflow token; the GitHub Release job receives
  job-scoped `contents: write`.
- npm package manifests: `repository.url` must match this GitHub repository for
  provenance. Package contract tests enforce it.
- GitHub environment `ci-approval`: must have the intended reviewers.
- `main` branch protection: should require the manual gate and every current CI
  job, and should block force-pushes and deletion.

Rotate registry credentials according to the repository's security policy. If
npm Trusted Publishing replaces the token in the future, update the workflow,
its executable contract, and this section together.
