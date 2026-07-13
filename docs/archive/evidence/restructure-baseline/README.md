# Restructure baseline

This directory freezes the same-machine baseline captured from
`pre-restructure` (`eb34bb09a5c8dadb050771edc8b5fb61ad6c4e16`) on 2026-07-12.
The host used macOS on Apple M1 Pro with Rust 1.97.0.

The complete workspace test suite passed before restructuring: 292 CLI/library
unit tests plus all CLI, rules, trace, example, and benchmark targets; the 1 GiB
resource test remained explicitly ignored as designed. The frozen external
contract owners were explicitly exercised:

- `rsproxy-cli/tests/cli_json_contracts.rs`
- `rsproxy-cli/tests/cli_product_matrix.rs` and `cli_product_matrix/`
- `rsproxy-cli/tests/cli_daemon_lifecycle.rs`
- `rsproxy-rules/tests/corpus.rs`
- `rsproxy-rules/tests/whistle_migration.rs`
- `rsproxy-rules/tests/whistle_options.rs`
- the 14 control tests then located under `rsproxy-cli/src/control/tests/`
- all nine `packages/npm/tests/*.test.js` contracts

The Rust contracts ran through
`cargo test --workspace --all-targets --locked`, with the action and protocol
owner inventories additionally verified by `./scripts/verify.sh actions` and
`./scripts/verify.sh matrix`. npm contracts ran through
`node --test packages/npm/tests/*.test.js`.

The short H1 proxy benchmark used 10,000 requests at concurrency 32 and measured
36,891.38 requests/second. This run supplements the established formal H1 target
of 45,392 requests/second recorded by the restructuring plan; final comparison
must use a same-machine rerun and accept no regression greater than 10%.

The pre-governance release binary was 15,548,144 bytes with SHA-256
`3cee07099813d725c753508d492c539ea60982618f3826be76bc9dd84a6c46e3`.

Files:

- `h1.json`: local H1 benchmark output.
- `criterion.json`: Criterion estimates for rules, trace, and cached TLS
  handshake targets.
