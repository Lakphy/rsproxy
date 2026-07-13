# Restructure Phase 2 evidence

This directory records the same-machine qualification run after extracting
`rsproxy-net`, `rsproxy-engine`, `rsproxy-control`, and `rsproxy-platform` from
the original CLI monolith. The run used macOS on Apple M1 Pro with Rust 1.97.0
on 2026-07-12.

## Performance

The H1 proxy benchmark used 10,000 requests at concurrency 32. It completed all
requests with zero status or I/O errors at 34,552.43 requests/second. The frozen
same-machine baseline was 36,891.38 requests/second, so the Phase 2 result is a
6.34% decrease and remains above the 10% regression floor of 33,202.25
requests/second.

Criterion was rerun for all 11 frozen rules, trace, and MITM certificate
metrics. Ten metrics were unchanged or faster; `trace_enqueue/abort_event` was
175.75 ns versus the 162.29 ns baseline, an 8.29% increase and within the 10%
acceptance band. The formal cached TLS target also passed at 277,315 ns against
the 3,000,000 ns ceiling.

The stripped thin-LTO release binary is 11,285,088 bytes with SHA-256
`94f86633d5985b3176a081feb02104665f51d974bb959abd7e560a2344893893`. The
pre-restructure binary was 15,548,144 bytes, a 27.42% reduction.

## Structure and contracts

The following gates passed from the completed Phase 2 tree:

- `cargo test --workspace --all-targets --locked`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `./scripts/check.sh all`
- `./scripts/verify.sh actions` (17 engine action-effect owners)
- `./scripts/verify.sh matrix` (34 protocol cases)
- all nine `packages/npm/tests/*.test.js` contracts
- Windows GNU workspace all-target check and warning-denied Clippy

Each extracted crate has a black-box public facade contract. Independent
reviews additionally verified dependency direction and API visibility; the
control wire no longer depends on the data-plane network crate; root CA PEM is
loaded only by the CLI composition root and injected into engine configuration;
and platform system-proxy APIs return typed plans/outcomes without presentation
data.

The local Linux musl check could not start third-party C compilation because
this macOS host does not provide `x86_64-linux-musl-gcc`; Windows cross-checks
and all host gates above completed successfully.

Files:

- `h1.json`: Phase 2 H1 proxy benchmark output.
- `criterion.json`: Phase 2 Criterion estimates for the frozen metric set.
