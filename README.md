# rsproxy

rsproxy is a programmable HTTP/HTTPS debugging proxy written in Rust. It can
intercept, inspect, rewrite, mock, trace, and replay traffic from a command-line
interface or terminal UI.

## Features

- HTTP/1.1, HTTP/2, HTTPS MITM, CONNECT tunnels, WebSocket, SSE, and gRPC
- An indexed rules DSL for routing, mocking, headers, bodies, cookies, delays,
  throttling, TLS policy, and trace control
- Bounded in-memory trace collection with optional compressed disk spill
- Foreground, daemon, and per-user login-startup modes, JSON/NDJSON output,
  HAR export, and a TUI
- Local CA management and native system-proxy integration with automatic
  routing restoration after login
- Native packages for macOS, Linux, and Windows behind one npm/Bun launcher

## Install

Node.js 18 or later is required by the launcher. The proxy itself runs as a
native executable and is not compiled during installation.

```sh
npm install --global @rsproxy/cli
# or
bun add --global @rsproxy/cli

rsproxy --version
```

Bun-only environments can run the same registry package with:

```sh
bunx --bun @rsproxy/cli --version
```

Supported native targets are macOS arm64/x64, Linux arm64/x64 with glibc or
musl, and Windows arm64/x64 with MSVC.

## Quick start

HTTP proxying works without a local CA:

```sh
rsproxy start
curl --proxy http://127.0.0.1:8899 http://example.com/
rsproxy trace ls
rsproxy stop
```

To inspect HTTPS traffic, initialize the local CA and review the trust-store
change before applying it:

```sh
rsproxy ca init
rsproxy ca install --dry-run
rsproxy ca install
rsproxy start
```

CA trust and system-proxy commands modify host operating-system state and may
require elevated privileges. Preview system-proxy changes with `--dry-run`:

```sh
rsproxy proxy on --all --dry-run
rsproxy proxy on --all
rsproxy tui

# Restore host state when finished.
rsproxy proxy off --all
rsproxy stop
```

Run `rsproxy help <COMMAND>` for command-specific options and examples.

## Start automatically at login

Preview the native login item, then install it. Automatic HTTP/HTTPS system
proxy routing is enabled by default and is applied only after the daemon reports
ready:

```sh
rsproxy startup install --dry-run
rsproxy startup install --start-now
rsproxy startup status
```

The registration is per-user: a LaunchAgent on macOS, the current-user `Run`
registry key on Windows, and XDG Autostart on Linux. Use `--service Wi-Fi` to
limit automatic routing to one macOS network service, or
`--no-system-proxy` to start only the daemon.

Uninstalling restores system proxy settings and stops the selected daemon before
removing the login item. `--keep-running` removes only future login startup.

```sh
rsproxy startup uninstall --dry-run
rsproxy startup uninstall
```

## Rules

Rules are evaluated in group order and then source order. This example mocks an
endpoint, adds a request header, and slows one response path:

```text
api.example.com/health mock("ok") res.type(text/plain)
**.example.com req.header(x-debug-proxy: rsproxy)
api.example.com/large throttle(res, 1MB/s)
```

Validate and install a rule group:

```sh
rsproxy rules check ./debug.rules
rsproxy rules set default --file ./debug.rules
rsproxy rules test https://api.example.com/health
```

See the [Rules DSL specification](docs/rules-dsl-spec.md) for the complete
matcher, action, condition, value-source, and template contracts.

## Common commands

```sh
rsproxy run                         # foreground mode
rsproxy start                       # background daemon
rsproxy status --json
rsproxy rules ls
rsproxy values ls
rsproxy trace follow                # live NDJSON
rsproxy trace export --har -o sessions.har
rsproxy replay 42                   # repeats the captured request and its side effects
rsproxy startup status
rsproxy completions zsh
rsproxy stop
```

The local control endpoint defaults to an owner-only Unix socket or an
authenticated Windows named pipe. TCP control endpoints configured with
`--api HOST:PORT` require an API token. Process logs always go to stderr;
request/session trace is a separate bounded data product.

## Build from source

The workspace requires Rust 1.88 or later.

```sh
cargo build --release -p rsproxy-cli --bin rsproxy --locked
cargo test --workspace --all-targets --locked
cargo xtask check all
```

The resulting executable is `target/release/rsproxy` (or `rsproxy.exe` on
Windows). See [Testing](docs/testing.md) for tool prerequisites and the complete
verification matrix.

## Documentation

- [Documentation index](docs/README.md)
- [Configuration](docs/configuration.md)
- [Rules DSL specification](docs/rules-dsl-spec.md)
- [Architecture](docs/architecture.md)
- [Testing](docs/testing.md)
- [Development and release process](docs/release-process.md)
- [npm/Bun distribution](packages/npm/README.md)

## Distribution

Releases publish ten npm packages: eight platform-specific native packages,
`@rsproxy/runtime`, and the shared `@rsproxy/cli` launcher. Version tags also
produce eight GitHub release archives and a `SHA256SUMS` manifest. Workspace
crates are not published to crates.io.

## License

[MIT](LICENSE)
