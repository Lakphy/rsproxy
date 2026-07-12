# Configuration

rsproxy resolves runtime settings in this order:

1. Command-line options.
2. A TOML configuration file.
3. Built-in defaults.

The default file is `$RSPROXY_HOME/config.toml` when `RSPROXY_HOME` is set,
otherwise `$HOME/.rsproxy/config.toml`. A missing default file is ignored.
`--config FILE` selects another file and reports an error if that explicit file
cannot be read or parsed.

Options follow the subcommand:

```sh
rsproxy run --config /etc/rsproxy.toml --port 8899
rsproxy status --config /etc/rsproxy.toml
rsproxy rules cat --config /etc/rsproxy.toml
```

The same resolved `api`, `storage`, token and proxy target settings are used by
daemon lifecycle commands, status, rules, values, trace, replay, CA commands,
the TUI and system-proxy commands. Configuration is loaded once per command and
is not hot-reloaded. The optional rule watcher described below reloads only the
rules directory, not this configuration file.

## Process logging

Process diagnostics use `tracing` and are written only to stderr, so commands
that emit JSON or NDJSON on stdout keep a clean machine-readable stream. Logging
is configured through environment variables rather than the runtime TOML file:

```sh
RSPROXY_LOG=rsproxy=debug RSPROXY_LOG_FORMAT=text rsproxy run
RSPROXY_LOG=rsproxy=info RSPROXY_LOG_FORMAT=json rsproxy run
```

`RSPROXY_LOG` takes precedence over `RUST_LOG`; a blank value falls through to
the next source, and the default is `rsproxy=info`. The supported formats are
`text` (also accepted as `compact`) and `json`. Stable events include
`daemon_started`, `proxy_listener_bound`, `control_listener_bound`, trust-root
loading, listener/connection failures, and session completion/failure. API and
proxy credentials are never logged; token authentication is represented only
by its secured token-file path.

## Fields

All fields are optional. Unknown fields are rejected so misspellings do not
silently fall back to defaults.

```toml
host = "127.0.0.1"
port = 8899
api = "127.0.0.1:8900" # or Unix "unix:/path/to/control.sock", Windows "pipe:NAME"
storage = "/home/user/.rsproxy"
watch = false
watch_debounce_ms = 200

# Secrets are optional. Prefer a 0600 file and avoid committing it.
api_token = "at-least-16-bytes"
proxy_auth = "user:password"

max_header_size = "256kb"
max_header_count = 256
body_buffer_limit = "8mb"

trace_body_limit = "64kb"
trace_filter = "media" # headers-only, media, or full
trace_segment_size = "64mb"
trace_disk_budget = "2gb" # 0 disables disk spill
trace_spill_compression = "none" # or zstd[:level]
no_trace_body = false

no_mitm = false
strict_mitm = false
mitm_cert_cache_capacity = 1024
mitm_failure_cache_capacity = 1024
mitm_failure_ttl_seconds = 300
connect_probe_timeout_ms = 250
h1_pool_max_active_per_key = 256
h1_pool_wait_timeout_ms = 15000
h2_pool_max_active_streams_per_key = 256
h2_pool_wait_timeout_ms = 15000

dns_timeout_ms = 5000
dns_cache_seconds = 60
dns_server = ["1.1.1.1", "127.0.0.1:5353"]
tcp_connect_timeout_ms = 10000
client_tls_handshake_timeout_ms = 10000
upstream_tls_handshake_timeout_ms = 10000
upstream_ttfb_timeout_ms = 60000
request_timeout_ms = 360000
```

Size fields accept a non-negative byte integer or a string with `b`, `kb`, `mb`
or `gb`. `body_buffer_limit` is the maximum body aggregated for request- or
response-body rules. An oversized HTTP/1 request is streamed unchanged: rules
that do not depend on its body still apply, body conditions and mutations are
skipped, and trace includes `request-body-rewrite-skipped-limit`. An oversized
response is forwarded unchanged and marked `body-rewrite-skipped-limit`. Pool
limits, this body limit, and timeout fields that represent active operations
must be greater than zero.
`dns_cache_seconds` and `trace_disk_budget` may be zero.

MITM defaults to `auto`: a CONNECT request is intercepted when the CA exists and
neither a `bypass` rule nor a remembered client TLS failure excludes the target.
`no_mitm = true` (or `--no-mitm`) globally selects passthrough. `strict_mitm =
true` (or `--strict-mitm`) keeps interception failures visible and disables the
failure-memory fallback. The two modes are mutually exclusive.

In auto mode, rsproxy sends the CONNECT 200 response and peeks without consuming
the first tunneled bytes. A TLS ClientHello enters MITM, recognizable plaintext
HTTP reuses the normal rule/forward/trace pipeline, and unknown traffic or probe
timeout is passed through. Because the 200 response and failed TLS handshake have
already consumed the first connection, a certificate-pinning failure cannot be
replayed on that same socket. Rsproxy remembers the host so the client's next
CONNECT retry passes through for `mitm_failure_ttl_seconds` (default 300 seconds).
The bounded LRU stores at most `mitm_failure_cache_capacity` hosts; zero disables
failure memory. `connect_probe_timeout_ms` must be positive. `/api/status` exposes
the active mode, capacities, active failure entries, TTL and probe timeout without
returning host names.

`watch = true` is equivalent to `--watch`: it monitors `<storage>/rules` for
external `*.rules` and `groups.toml` changes. Changes are grouped using a 200ms
trailing-edge debounce by default; `watch_debounce_ms` or
`--watch-debounce-ms` selects another positive duration. Each batch reloads and
validates all groups before one atomic snapshot publication. Invalid edits keep
the previous snapshot active and are reported under `rule_watch.last_error` in
`/api/status`. The status object also reports event, dropped-event, successful
reload and failure counters.

For TCP and Windows named-pipe control clients, token resolution is `--api-token` >
`RSPROXY_API_TOKEN` > `api_token` in TOML > `<storage>/run/api-token`. Unix
control sockets use peer/file permissions and ignore token settings. Windows
defaults to `pipe:rsproxy-control`; the first pipe instance is exclusive,
remote clients are rejected, and the local pipe still requires the storage
token. Unix defaults to `<storage>/run/ctl.sock`; when that path would exceed
the conservative `sun_path` limit, rsproxy deterministically uses a short
UID+storage-hash socket under `/tmp`, still with mode 0600, and removes it on
normal stop or failed startup. Status reports the loaded configuration path and package version but
never returns either secret.
