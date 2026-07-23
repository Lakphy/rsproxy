# Rule language v2 to v3 migration

V3 makes the source grammar explicit and removes compatibility aliases from
runtime-loaded rule files. Every non-empty standalone or persisted group must
start with `@language 3` on its first non-comment, non-blank line.

Use the built-in migration before validating or installing an older file:

```sh
rsproxy rules migrate old.rules > rules-v3.rules
rsproxy rules check rules-v3.rules
rsproxy rules migrate old.rules --write
```

The migration preserves comments, quoted values, and regex bodies, rewrites
known call aliases, adds/replaces the language directive, and validates the
result with the strict v3 parser before output. `--write` uses a temporary file
and rename so an invalid or interrupted migration does not partially overwrite
the input.

The principal canonical-name changes are:

| V2 compatibility spelling | V3 spelling |
| --- | --- |
| `clientIp(...)`, `client_ip(...)`, `client-ip(...)`, `ip(...)` | `client.ip(...)` |
| `serverIp(...)`, `server_ip(...)`, `server-ip(...)` | `server.ip(...)` |
| `resHeader(...)`, `res_header(...)`, `res-header(...)` | `res.header(...)` |
| `mockRaw(...)`, `mock_raw(...)`, `mock-raw(...)` | `mock.raw(...)` |
| `mapRemote(...)`, `map_remote(...)`, `map-remote(...)` | `map.remote(...)` |

The language version also includes stricter safety semantics: malformed URL
ports and authorities are rejected, unsafe redirect locations are rejected,
message-framing and routing fields cannot be created as response trailers,
and HTTP 205 upstream content is consumed and discarded.

Rust integrations that construct short rule strings in code may continue to
use `RuleSet::parse`; it compiles current semantics while accepting the v2
aliases. Files and persisted groups should use `RuleSet::parse_versioned` (or
`parse_versioned_groups`) so deployment behavior matches the CLI and daemon.
