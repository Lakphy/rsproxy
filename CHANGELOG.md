# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.2] - 2026-07-14

### Added

- Added human-first output for daemon status, rules and values mutations,
  Trace details and statistics, and Replay while preserving stable `--json`
  output for scripts.
- Added `config show` and `config path`, discoverable command aliases,
  list-by-default behavior for Trace, rules, and values, and
  `trace replay <ID>` alongside the top-level Replay command.
- Added reliable HTTP and HTTPS Replay with shared DNS and timeout settings,
  bounded response previews, and correct handling for content-length,
  chunked, close-delimited, and bodyless responses.

### Changed

- Expanded the CLI help and quick-start guidance, tightened `stop` and
  `status` to accept only relevant options, and made error reporting distinguish
  control timeouts, daemon failures, and unreachable endpoints.
- Reorganized the maintained documentation around current executable
  contracts, corrected configuration and rules scope details, and removed
  obsolete implementation plans and dated qualification reports.
- Hardened the release SOP with lessons from the v0.0.1 launch: a
  failure-recovery playbook (when a tag may be moved versus when a version
  is frozen), registry propagation guidance, the provenance manifest
  contract, and a mandatory packaging drill before tagging.

### Fixed

- Prevented the npm launcher from orphaning a native proxy process by
  forwarding termination signals, watching for launcher death, surfacing bind
  failures, and safely reclaiming only verified orphaned rsproxy listeners.
- Rejected malformed quoted or otherwise invalid HTTP header names in rules
  instead of emitting invalid fields on proxied traffic.

## [0.0.1] - 2026-07-13

Initial public release.

### Added

- Debugging proxy engine with a rules DSL for matching and rewriting traffic,
  HTTP/1.1, HTTP/2, WebSocket, and TLS MITM support, trace capture, and a
  CLI/TUI frontend (`rsproxy`) built from eight focused workspace crates.
- npm distribution: eight native platform packages
  (`@rsproxy/darwin-arm64`, `@rsproxy/darwin-x64`,
  `@rsproxy/linux-{x64,arm64}-{gnu,musl}`,
  `@rsproxy/win32-{x64,arm64}-msvc`), the `@rsproxy/runtime` resolver, and
  the unified `@rsproxy/cli` launcher for Node and Bun — all published with
  npm provenance.
- GitHub Releases on every version tag with one binary archive per target,
  a `SHA256SUMS` manifest, and notes extracted from this changelog.
- Typed domain errors across every crate, versioned JSON error schemas with
  stable exit-code classes, and public-API snapshots gating each library
  boundary.
- Engineering automation via `cargo xtask`: repository structure, workflow,
  typed-error, and API checks; release version synchronization; coverage and
  performance target validation.

### Security

- Locked cargo-deny advisory, license, dependency-ban, and registry-source
  checks in CI; workflow permissions are read-only except the job-scoped
  release grant; Dependabot keeps cargo, npm, and github-actions
  dependencies patched.
