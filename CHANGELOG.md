# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
