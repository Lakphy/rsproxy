# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added `rsproxy rules help` with a complete searchable 77-topic offline
  language index, parser-authoritative DSL spellings, action resolution modes,
  fixed limits, and the versioned `rsproxy.rules.help/v1` JSON contract.
- Added a shared public rules-language registry, explicit language version 3,
  strict `@language 3` standalone/persisted sources, canonical-only v3 call
  names, `rsproxy rules migrate`, and v2-to-v3 source/Rust-API migration notes.
- Added the `map.remote(url)` action: a transparent reverse proxy (Whistle
  `pattern target` / Charles Map Remote equivalent) that serves the request
  from the target backend without a client-visible redirect. Targets without a
  path keep the original path and query; explicit target paths support matcher
  captures such as `$1`. Sessions gain a `map-remote` trace flag. (#12)
- Added `rsproxy rules lint`, which reports later rules that can never win a
  single-action family because an earlier, condition-free, broader rule always
  matches first, repeated single-action families within one rule, and provably
  contradictory positive/negative method, status, environment, and constant
  chance/boolean conjunctions, and request-only actions guarded by conditions
  that necessarily require response metadata. It
  also catches same-rule actions suppressed by `skip`, conflicting local
  responses, response actions bypassed by local responses, and upstream routes
  overridden by `direct`, plus body actions made ineffective by
  `res.status(204/205/304)`. Action help now exposes effect phases. Its JSON
  contract is versioned as `rsproxy.rules.lint/v1`, and findings produce a
  non-zero exit.
  The rules DSL spec now documents first-match-wins ordering prominently. (#12)
- Added `all(...)` and `not(...)` condition combinators alongside `any(...)`
  and the `!` prefix. (#12)
- Added the inline mock form
  `mock(status=..., type=..., header=Name: value, body=...)` for one-stop
  status/header/body mocks. (#12)
- Added migration hints for raw Whistle operator tokens: `socks://...`,
  `proxy://...`, `http(s)://...`, `host:port`, and `$0` now fail with the
  equivalent rsproxy rule spelled out. (#12)
- Added `startup install`, `startup status`, and `startup uninstall` with
  per-user macOS LaunchAgent, Windows Run-key, and Linux XDG Autostart backends.
  Login startup waits for the daemon to become ready before restoring native
  HTTP/HTTPS system proxy routing, supports dry runs and JSON status, and safely
  disables routing and stops the daemon during normal uninstall.

### Changed

- Rule snapshots now own deduplicated precompiled glob programs and expose the
  count through `rules stats`/`rules bench`; `RuleSet` inspection uses read-only
  accessors so its AST cannot diverge from compiled indices after publication.
- Language v3 rejects empty argument slots, surplus redirect/attachment
  arguments, unknown `skip` families, empty source tags, non-finite timing/rate
  values, invalid glob escapes, invalid environment names, unbalanced DSL
  delimiters, status conditions outside `100..599`, final statuses outside
  `200..599`, and redirect statuses other than 301/302/303/307/308.
- Body substring conditions are deduplicated into one case-insensitive compiled
  matcher per snapshot and scan each request body once per resolution. Rule
  statistics expose the compiled body-literal count.
- Snapshot compilation now caps aggregate source, groups, rules, diagnostics,
  snapshot/per-rule actions and conditions, per-rule properties, and call arguments. Snapshot versions
  are process-local monotonic IDs, unique across concurrent publications and
  immune to wall-clock rollback.

### Security

- Replaced recursive glob backtracking with anchored linear regex programs,
  bounded captures, source-line and parse-depth limits, and publication-time
  validation to prevent stack exhaustion and exponential matching behavior.
  Aggregate snapshot and per-rule limits bound many-small-input amplification,
  CLI/storage reads enforce those budgets before unbounded allocation, and
  environment condition names containing NUL are rejected before runtime.
  External rule values and mock files are capped at 8 MiB, PEM inputs at 1 MiB,
  and their CLI, control-plane, and execution-time readers reject overflow
  before allocating the complete file. Template/capture/regex expansion is
  length-checked before allocation, and rule-produced paths, trace tags, URLs,
  headers, cookies, bodies, injections, and JSON merges obey explicit aggregate
  output budgets. Resolved provenance/capture and lint-source clones share
  immutable storage; lint reports expose completeness when their comparison
  count, charged matcher-byte, finding, or report-byte budget is reached.
  Dynamically rendered header/trailer values, methods, rewritten URLs, redirect
  locations, and mock response fields are protocol-validated before
  serialization, preventing CRLF injection through templates, references,
  files, or raw mocks. HTTP response serialization owns framing headers and
  enforces HEAD/204/205/304 body semantics. Malformed upstream 205 content is
  consumed and discarded, forbidden/connection-nominated trailers are removed
  on every forwarding path, attachment filenames use quoted escaping plus
  UTF-8 extended values, strict URLs reject malformed ports and authorities,
  and directory mocks use capability-relative file handles to reject traversal
  and symlink races. Body predicates share one decoded request view per resolution
  and are capped per snapshot to bound regex scans.

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
