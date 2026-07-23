# Architecture

rsproxy is a Rust workspace split by domain. The executable is a composition
root; protocol, policy, persistence, control, and operating-system concerns are
kept behind crate boundaries.

## Extension safety boundary

The rule engine publishes typed, immutable actions; the proxy engine owns their
bounded effects. It deliberately does not execute arbitrary host-language code
from `script://`, `resScript://`, or a similar in-process hook. That boundary
keeps action phase, body planning, protocol framing, lint, and output budgets
visible before a ruleset is published.

A future general-purpose transform must be a separately versioned sandbox
component rather than another untyped `Action` payload. Its contract must:

- receive a bounded request or response snapshot and return a typed patch;
- deny filesystem, process, and network capabilities by default;
- enforce wall-time, instruction/fuel, memory, input, and output limits;
- make capability grants explicit in configuration, not rule text; and
- fail one transform without compromising the proxy process or framing state.

Typed actions remain the preferred extension path for common migrations such as
CORS, header/body changes, JSON merge, and content injection.

## Workspace map

| Path | Responsibility | Must not own |
| --- | --- | --- |
| `crates/rsproxy-rules` | Parse, validate, index, and resolve the rules DSL | Filesystem value loading, network I/O, or HTTP mutation |
| `crates/rsproxy-trace` | Assemble bounded session events, retain sessions, and read/write spill segments | Socket observation, rule execution, control APIs, or redaction policy |
| `crates/rsproxy-net` | HTTP framing, downstream/upstream HTTP/2, DNS, async I/O, deadlines, and pool admission | Application configuration, rules, certificates, or trace retention |
| `crates/rsproxy-engine` | Compose network transport, rule effects, TLS interception, tracing, replay, and rule storage | CLI parsing, control routing, or host operating-system mutation |
| `crates/rsproxy-control` | Bind the local control transport, authenticate requests, expose API routes, and provide a client | Proxy data-plane policy or CLI rendering |
| `crates/rsproxy-platform` | Root-CA storage/trust, process helpers, resident-memory inspection, socket naming, login-startup registration, and system-proxy operations | Proxy traffic, rule execution, or CLI presentation |
| `crates/rsproxy-cli` | Parse commands/configuration, compose the runtime, manage daemon lifecycle, render output, and run the TUI | Reimplement lower-layer protocol or policy logic |
| `crates/xtask` | Repository contracts, public-API snapshots, version synchronization, and report validation | Product runtime behavior |
| `packages/npm` | Native target mapping and the shared npm/Bun launcher | Rust compilation at installation time |

Each library crate exposes a documented public facade from `src/lib.rs`.
Implementation modules remain private unless another crate genuinely needs the
abstraction.

## Dependency direction

The current internal dependency graph is:

```text
rsproxy-cli
├── rsproxy-control
│   ├── rsproxy-engine
│   │   ├── rsproxy-net
│   │   ├── rsproxy-rules
│   │   └── rsproxy-trace ──> rsproxy-rules
│   ├── rsproxy-rules
│   └── rsproxy-trace
├── rsproxy-engine
├── rsproxy-platform
├── rsproxy-rules
└── rsproxy-trace
```

`rsproxy-net`, `rsproxy-platform`, and `rsproxy-rules` are leaf domain crates:
they do not depend on another rsproxy crate. The control plane reaches mutable
engine state through `EngineHandle`; it does not import engine internals.

Use `cargo metadata --no-deps --format-version 1` when checking this graph. The
crate manifests, not a hand-maintained diagram, are authoritative.

## Runtime composition

`rsproxy-cli` resolves built-in defaults, a TOML file, and CLI overrides into an
`AppConfig`. It loads root-CA material through `rsproxy-platform`, injects that
material into `rsproxy-engine::ProxyConfig`, creates `SharedState`, and starts:

1. the proxy listener owned by `rsproxy-engine`; and
2. the local control listener owned by `rsproxy-control`.

`SharedState` is the data-plane ownership root. Its clones share immutable
configuration, atomically published rule snapshots, the trace collector, DNS
resolver, connection admission state, and bounded MITM caches. `EngineHandle`
is the narrower control-plane view used for status, rule updates, trace access,
and replay.

Configuration is resolved once at process start. Only rule files and group
metadata can be watched and atomically reloaded.

## Request path

The exact path differs by HTTP version and CONNECT mode, but the policy stages
are consistent:

1. Accept a downstream connection and enforce proxy authentication and header
   limits.
2. Classify ordinary HTTP, CONNECT passthrough, plaintext HTTP in a tunnel, or
   TLS eligible for MITM.
3. Build immutable request metadata and resolve request-phase rules.
4. Apply control-flow, route, URL, header, cookie, body, delay, throttle, and TLS
   actions. Body-dependent operations aggregate only up to the configured
   buffer limit; oversized bodies continue on a streaming path.
5. Connect directly or through the selected upstream proxy chain, with DNS,
   pool-admission, stage timeout, and total-deadline enforcement.
6. Resolve response-phase conditions, apply response actions, and stream the
   result downstream.
7. Emit bounded trace events throughout the lifecycle and finalize or abort the
   session exactly once.

`rsproxy-rules` returns typed actions but never performs effects. The engine is
the single owner of translating those actions into protocol behavior.

## HTTPS and CONNECT

MITM is enabled automatically when root-CA material exists unless global
`no_mitm`, a matched `bypass` action, or remembered client TLS failure selects
passthrough. Strict mode reports interception failures instead of remembering a
host and falling back on its next connection.

The engine peeks at initial tunnel bytes without consuming them. TLS enters the
MITM path, recognizable HTTP reuses the normal pipeline, and unknown traffic or
probe timeout passes through. Leaf certificates are issued and cached by the
engine; root-CA persistence and trust-store mutation remain platform concerns.

## Rules and values

`RuleStore` loads ordered groups from `<storage>/rules`:

```text
rules/
├── groups.toml
├── default.rules
└── <group>.rules
```

Parsing produces an immutable indexed `RuleSet`. A valid reload publishes one
complete snapshot; an invalid edit leaves the previous snapshot active. Named
values live in `<storage>/values`. Rules classify `@key` and `<path>` sources;
the engine loads them only when applying a matched action.

The [Rules DSL specification](rules-dsl-spec.md) is tied to a machine-readable
corpus. Do not introduce a second informal action list in architecture docs.

## Trace model

The proxy path emits lifecycle events to `rsproxy-trace`; a dedicated collector
assembles completed `Session` values. Queue capacity, total memory, body
previews, follower count, and disk spill are bounded. Disk spill uses
append-only NDJSON segments with optional per-record zstd compression and
oldest-first budget eviction.

The trace crate stores what callers give it. The engine decides capture policy,
and the control/CLI layers own authenticated export and presentation.

## Control plane

The default control transport is local:

- Unix: owner-only Unix-domain socket, with a deterministic short-path fallback
- Windows: local named pipe authenticated with the storage token
- Optional TCP: `HOST:PORT`, always authenticated with an API token

The API exposes status, rules, values, sessions, trace operations, replay, and
the public root certificate. CLI machine output and errors are presentation
contracts layered above the API; process logging remains on stderr.

## Persistent state

The selected storage directory contains product state:

```text
<storage>/
├── config.toml       # optional default configuration
├── ca/               # root and cached leaf certificates
├── rules/            # ordered rule groups
├── values/           # named rule values
├── trace/            # optional spill segments
└── run/              # pid, log, token, and local endpoint state
```

Callers should treat this layout as application-owned. Use CLI commands to
modify rules, values, CA trust, and daemon state where possible.

Per-user login registration lives in the operating system's standard startup
location rather than `<storage>`. A small versioned startup manifest in the
platform user-configuration directory points back to the selected storage and
runtime config. The login entry invokes only the hidden launcher; the launcher
starts the normal daemon, waits for readiness, and then applies system proxy
routing, keeping startup policy out of the proxy engine.

## Repository invariants

Workspace lints deny unsafe code by default, missing documentation,
unreachable public items, broad Clippy warnings, `unwrap`, debug macros, TODOs,
and unimplemented placeholders. Platform and Windows control adapters contain
narrow, documented unsafe exceptions at their crate boundaries.

Repository checks also enforce:

- Rust source files at or below the configured line limit
- dedicated test placement and a public integration-test boundary per crate
- typed errors across workspace and fuzz sources
- public-API snapshots for library crates
- exact workflow inventory, triggers, permissions, action pins, and commands

Run `cargo xtask check all`; see [Testing](testing.md) for prerequisites and
specialized suites.
