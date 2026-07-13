# Restructure final evidence

This directory records the final restructuring evidence dated 2026-07-12. The
host was macOS 27.0 on Apple M1 Pro (`aarch64-apple-darwin`) with Rust 1.97.0.
The frozen comparison point is `pre-restructure`
(`eb34bb09a5c8dadb050771edc8b5fb61ad6c4e16`).

## Results

| Check | Baseline / target | Final observation | Result |
| --- | ---: | ---: | --- |
| H1 throughput, 10,000 requests at concurrency 32 | 36,891.38 rps; floor 33,202.25 rps | 38,017.03 rps (+3.05%); p50 710 us, p99 2,779 us, zero errors | PASS |
| Cached TLS Criterion absolute target | upper bound < 3,000,000 ns | upper bound 265,942.96 ns | PASS |
| Frozen Criterion regression, 10% tolerance | all 11 metrics <= +10% | 10/11 pass; `trace_enqueue/abort_event` was 306.74 ns versus 162.29 ns (+89.01%) | FAIL (host-load qualification blocked) |
| Paired trace comparison under the same host load | current no slower than `pre-restructure` by >10% | median 460.97 ns versus 620.88 ns (-25.75%); current was faster in all three pairs | PASS |
| Release binary | 15,548,144 bytes | 11,964,432 bytes (-23.05%) | PASS |

The H1 run completed all 10,000 requests, returned 10,240,000 response bytes,
and reported neither status nor I/O errors. Its throughput is 14.50% above the
10% regression floor.

The release artifact identifies itself as `rsproxy 0.2.0`. Its SHA-256 is
`8004c33b6191006987d183ddca60e83ad7aacf9ae0494b1d838d8d1fe853afb3`.

## Criterion qualification

The complete Criterion collection met the cached-TLS absolute target and 10 of
the 11 frozen regression comparisons. The trace enqueue result did not meet the
frozen-report gate, so this directory deliberately records the command as a
failure rather than relabeling it as a pass.

The host was not capable of reproducing the frozen trace timing during final
collection. After project-side Cargo and benchmark processes were stopped, the
8-core host still had load averages ranging from 7.4 to 44, with Spotlight and
Warp commonly consuming roughly 50-74% CPU each. Repeated current trace runs
varied from about 306 to 709 ns. More importantly, the unchanged
`pre-restructure` benchmark ran at 615-648 ns in the same window, far from its
frozen 162.29 ns result.

To distinguish a code regression from host scheduling noise, the baseline tag
and current tree were run back-to-back for three alternating pairs. Each run
used 200 samples and an 8-second measurement interval; no single run was
selected or discarded:

| Pair | `pre-restructure` | Current | Change |
| --- | ---: | ---: | ---: |
| 1 | 620.88 ns | 457.70 ns | -26.28% |
| 2 | 614.66 ns | 460.97 ns | -25.00% |
| 3 | 648.11 ns | 610.86 ns | -5.75% |
| Median | 620.88 ns | 460.97 ns | -25.75% |

`git diff --exit-code pre-restructure -- crates/rsproxy-trace/src
crates/rsproxy-trace/benches/trace.rs` was clean: the trace implementation and
benchmark have no behavioral diff from the baseline tag. The paired result is
therefore the qualified same-load comparison, while the frozen JSON command
remains explicitly environment-blocked and failing in `criterion-checks.txt`.

## Commands

```sh
RSPROXY_BENCH_REQUESTS=10000 RSPROXY_BENCH_CONCURRENCY=32 \
  ./benches/e2e/benchmark.sh \
  > docs/archive/evidence/restructure-final/h1.json

./benches/criterion/run.sh \
  docs/archive/evidence/restructure-final/criterion.json

cargo xtask targets criterion \
  docs/archive/evidence/restructure-final/criterion.json

cargo xtask targets regression \
  docs/archive/evidence/restructure-baseline/criterion.json \
  docs/archive/evidence/restructure-final/criterion.json 10

cargo bench -p rsproxy-trace --bench trace --locked -- \
  --noplot --sample-size 200 --measurement-time 8

cargo build --release -p rsproxy-cli --bin rsproxy --locked
wc -c < target/release/rsproxy
shasum -a 256 target/release/rsproxy
```

The paired baseline used the same `cargo bench` command from a detached
`pre-restructure` worktree. Pair order was baseline/current, current/baseline,
then baseline/current.

## Structural snapshot

- 8 workspace members.
- CLI production surface: 4,365 lines across 28 Rust files.
- 10 root orchestration scripts and 8 task scripts.
- 27 typed `public_api` test cases.
- 35 D-15 tests moved to public integration boundaries.
- Largest Rust source file: `cli/daemon.rs`, 458 lines.

## Files

- `h1.json`: final H1 benchmark output.
- `criterion.json`: first complete final Criterion collection.
- `criterion-checks.txt`: exact absolute-target and frozen-regression outcomes.
- `trace-paired.json`: the three extended, alternating same-load trace pairs.
- `release-binary.txt`: final release version, size, and SHA-256.
