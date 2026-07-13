# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Publish a GitHub release on every version tag with one binary archive per
  target, a `SHA256SUMS` manifest, and notes extracted from this changelog,
  created only after the npm packages publish successfully.
- Added Dependabot updates for cargo (weekly, grouped), npm, and
  github-actions dependencies, all validated by the existing CI gates.
- Documented the development and release standard operating procedure in
  `docs/release-process.md` (bilingual English/Chinese).

### Changed

- CI now runs once per change: pushes trigger it only on `main`, pull
  requests and merge groups cover branches, and superseded PR runs are
  cancelled through a concurrency group.
- Split the serial repository-contract CI job into parallel lint,
  repository-contract, and distribution jobs to shorten the critical path.
- Serialized release workflow runs with a concurrency group and extended the
  workflow contracts to scope `contents: write` to the GitHub-release job.
- Decoupled the performance and fuzz workflows from pushes and PRs: both run
  on daily schedules plus manual dispatch and must pass before every release.
- Switched CI test execution to a pinned cargo-nextest, dropped the
  rarely-hit release-matrix Rust caches so the repository cache quota serves
  hot PR caches, and tightened CI job timeouts.
- Gated PR CI runs behind manual approval (`ci-approval` environment): runs
  wait at no cost until approved from the PR page, and branch protection on
  `main` requires the gate plus every CI job before merging.

## [0.2.0] - 2026-07-12

### Added

- Split the workspace into eight focused crates: rules, trace, network protocol
  primitives, proxy engine, control plane, platform integration, CLI, and
  engineering automation.
- Added typed domain errors across every Rust crate and a single CLI renderer
  for human and versioned JSON errors with stable exit-code classes.
- Added `cargo xtask release`, cross-platform repository checks, typed
  performance-target validation, and public-API integration tests for each
  library boundary.

### Changed

- Replaced handwritten CLI parsing, help, and completion generation with typed
  `clap` derives. Help and completion formatting changed, and unknown or
  misspelled arguments now return usage errors instead of being ignored.
- Centralized workspace dependencies, lints, MSRV, release optimization, and
  Rust/npm version synchronization; the binary package is now `rsproxy-cli`
  while the installed command remains `rsproxy`.
- Reduced shell entry points to process orchestration only; repository policy
  and report validation now run through `cargo xtask` on every supported CI OS.
- Preserved the versioned CLI JSON schemas, rules DSL corpus, control API, trace
  contracts, npm package names, and launcher/runtime installation model.

### Security

- Added locked cargo-deny license, advisory, dependency-ban, and source checks,
  and retained npm provenance for all native and launcher package publishes.
