# Contracts hardening baseline

This baseline fixes the comparison point at commit
`09de1fef3af12844d2346df6bd00bc1ba2d29c2c`. Measurements were taken on
2026-07-13 on an Apple Silicon Mac (`aarch64-apple-darwin`, Darwin 27.0.0),
with `rustc 1.97.0 (2d8144b78 2026-07-07)` and Cargo 1.97.0.

The timed commands ran from `git archive HEAD` in `/tmp`, at reduced process
priority, with a distinct initially empty `CARGO_TARGET_DIR` for each command.
This both makes "cold" precise and avoids contending for the working tree's
Cargo lock. Registry and compiler caches were not cleared, so future
comparisons must use the same convention.

## Build and test timing

| Measurement | Command | Result |
| --- | --- | ---: |
| Cold workspace build | `cargo build --workspace --timings --quiet` | 47.95 s wall, 164.77 s user, 32.62 s sys |
| Cold workspace test | `cargo test --workspace --no-fail-fast --quiet` | 99.09 s wall, 246.97 s user, 48.88 s sys |

The test run passed: 544 tests were discovered, 543 passed, and the 1 GiB
resource test was ignored as designed. Cargo's timing report was generated in
the isolated build target at `cargo-timings/cargo-timing.html`; it is transient
machine-local evidence, while the wall-clock result above is the comparison
number retained in the repository.

Reproduction setup:

```sh
baseline=$(mktemp -d /tmp/rsproxy-hardening-baseline.XXXXXX)
git archive 09de1fef3af12844d2346df6bd00bc1ba2d29c2c | tar -x -C "$baseline"
cd "$baseline"
/usr/bin/time -p env CARGO_TARGET_DIR="$baseline/target-build" nice -n 10 \
  cargo build --workspace --timings --quiet
/usr/bin/time -p env CARGO_TARGET_DIR="$baseline/target-test" nice -n 10 \
  cargo test --workspace --no-fail-fast --quiet
```

## Rustdoc surface indicator

These are lexical line counts, not a percentage of semantically documented
API items. "Doc lines" matches `^[[:space:]]*///`; "public declarations"
matches an unqualified `pub` followed by `fn`, `struct`, `enum`, `trait`,
`type`, `const`, `static`, `mod`, or `use`. Public fields and enum variants are
therefore intentionally not counted. This simple indicator is stable enough
for a before/after comparison; the later `missing_docs` gate is authoritative.

| Crate | `///` lines | Public declaration lines |
| --- | ---: | ---: |
| rsproxy-rules | 6 | 103 |
| rsproxy-trace | 0 | 58 |
| rsproxy-net | 32 | 114 |
| rsproxy-platform | 19 | 69 |
| rsproxy-engine | 31 | 57 |
| rsproxy-control | 32 | 24 |
| rsproxy-cli | 38 | 74 |
| xtask | 11 | 28 |
| **Workspace** | **169** | **527** |

Reproduce either column against the fixed commit with `git grep`, for example:

```sh
git grep -E '^[[:space:]]*///' 09de1fef -- crates/rsproxy-rules/src | wc -l
git grep -E '^[[:space:]]*pub[[:space:]]+((async|unsafe|const)[[:space:]]+)*(fn|struct|enum|trait|type|const|static|mod|use)([[:space:]]|$)' \
  09de1fef -- crates/rsproxy-rules/src | wc -l
```

## Production `unwrap` / `expect` inventory

The product crates contain exactly 50 call sites after excluding paths named
`tests`, `tests.rs`, `test_support`, or `test_support.rs`. The inventory is
fixed to the baseline commit, so line numbers remain meaningful while the
hardening work edits the working tree.

| Crate | Count | Baseline locations (`file:line`) |
| --- | ---: | --- |
| rsproxy-control | 3 | `client.rs:16,27`; `shapes/har.rs:28` |
| rsproxy-engine | 21 | `handle.rs:72`; `proxy/forward.rs:124,151`; `proxy/h1_forward/pool.rs:124`; `proxy/server/connect_policy.rs:32`; `proxy/server/mitm.rs:50,101`; `proxy/tls/config.rs:11,29`; `proxy/websocket/concurrent.rs:53,87`; `rule_store.rs:100,118,136,165`; `rule_store/watch.rs:67,72,86,95,104,115` |
| rsproxy-net | 16 | `downstream_h2/message.rs:177`; `runtime.rs:17`; `upstream_body.rs:89,94`; `upstream_h2.rs:101,158,166,177`; `upstream_h2/connection.rs:63`; `upstream_h2/pool.rs:109,174,226,234`; `upstream_h2/streaming.rs:123,131,142` |
| rsproxy-platform | 1 | `ca/storage.rs:199` |
| rsproxy-rules | 4 | `template/metadata.rs:29,76,129`; `template/transform.rs:158` |
| rsproxy-trace | 5 | `spill.rs:128`; `store.rs:135,325,326,347` |
| **Product crates** | **50** | |

Reproduction command:

```sh
git grep -n -E '\.(unwrap|expect)\(' 09de1fef -- \
  'crates/rsproxy-*/src/**/*.rs' 'crates/rsproxy-*/src/*.rs' |
  grep -Ev '/(tests|test_support)(/|\.rs:)'
```

`xtask` has four additional production call sites and is excluded from the 50
because the panic-policy phase scopes its product-code inventory to
`rsproxy-*`.

## Integration-test binaries

The seven product packages have 33 top-level `tests/*.rs` targets containing
98 tests. Counts come from Cargo's test harness (`-- --list`), so tests pulled
in through nested modules are included.

| Crate | Binary | Tests |
| --- | --- | ---: |
| rsproxy-rules | complexity | 2 |
| rsproxy-rules | corpus | 1 |
| rsproxy-rules | fuzz_seeds | 1 |
| rsproxy-rules | properties | 3 |
| rsproxy-rules | public_api | 2 |
| rsproxy-rules | value_matrix | 1 |
| rsproxy-rules | value_sources | 3 |
| rsproxy-rules | whistle_migration | 1 |
| rsproxy-rules | whistle_options | 1 |
| rsproxy-trace | public_api | 3 |
| rsproxy-net | dns | 4 |
| rsproxy-net | errors | 2 |
| rsproxy-net | http_buffered_head | 3 |
| rsproxy-net | http_tcp_head | 3 |
| rsproxy-net | public_api | 7 |
| rsproxy-net | request_deadline | 3 |
| rsproxy-platform | ca | 5 |
| rsproxy-platform | errors | 3 |
| rsproxy-platform | process | 2 |
| rsproxy-platform | public_api | 5 |
| rsproxy-engine | errors | 5 |
| rsproxy-engine | public_api | 5 |
| rsproxy-engine | rule_store | 5 |
| rsproxy-control | public_api | 3 |
| rsproxy-cli | cli_completions | 2 |
| rsproxy-cli | cli_daemon_lifecycle | 5 |
| rsproxy-cli | cli_help | 6 |
| rsproxy-cli | cli_json_contracts | 4 |
| rsproxy-cli | cli_logging | 1 |
| rsproxy-cli | cli_product_matrix | 4 |
| rsproxy-cli | cli_rule_groups | 1 |
| rsproxy-cli | cli_trace_follow | 1 |
| rsproxy-cli | large_stream_resource | 1 |
| **Total** | **33 binaries** | **98** |

For each package, reproduce the harness counts from the isolated test target:

```sh
CARGO_TARGET_DIR="$baseline/target-test" cargo test -p rsproxy-rules \
  --tests -- --list
```

The workspace also has one `xtask/tests/public_api.rs` integration target with
two tests; it is not one of the 33 product integration-test binaries used as
the Phase 6 comparison group.
