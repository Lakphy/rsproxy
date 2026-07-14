# Documentation

This directory contains the maintained documentation for the current rsproxy
implementation.

| Document | Purpose |
| --- | --- |
| [Configuration](configuration.md) | Configuration precedence, fields, defaults, logging, control endpoints, and runtime limits |
| [Rules DSL specification](rules-dsl-spec.md) | Executable matcher, action, condition, value, template, and error contracts |
| [Architecture](architecture.md) | Workspace boundaries, dependency direction, runtime data flow, and persistent state |
| [Testing](testing.md) | Local checks, CI jobs, specialized acceptance suites, fuzzing, and benchmarks |
| [Development and release process](release-process.md) | Contribution workflow, versioning, packaging, publishing, and release recovery |

The root [README](../README.md) is the product entry point. Package-specific
distribution details live in [packages/npm](../packages/npm/README.md), and
release history lives in the root [changelog](../CHANGELOG.md).

## Sources of truth

When documentation and implementation disagree, update the documentation in
the same change as the authoritative contract:

- CLI commands and flags: `rsproxy --help` and
  `crates/rsproxy-cli/src/cli/command*.rs`
- Runtime configuration: `crates/rsproxy-cli/src/cli/config*` and
  `crates/rsproxy-engine/src/state.rs`
- Rules behavior: `docs/rules-dsl-spec.md`, the rules corpus, and
  `Action::FAMILIES`; the corpus test checks the specification anchors
- Workspace boundaries: crate manifests and public `lib.rs` facades
- CI and release behavior: `.github/workflows/`, guarded by
  `cargo xtask check workflows`
- Package and target inventory: `packages/npm/targets.json` and package tests

The Markdown files under
`crates/rsproxy-rules/tests/fixtures/whistle-2.10.5/` are immutable upstream
test fixtures. They are inputs to compatibility tests, not rsproxy
documentation, and should not be edited as part of documentation maintenance.
