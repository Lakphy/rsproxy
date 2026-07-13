# Contracts hardening final evidence

This directory records the final contracts-hardening evidence dated
2026-07-13. The checks were run on macOS 27.0 on Apple Silicon
(`aarch64-apple-darwin`) with `rustc 1.97.0 (2d8144b78 2026-07-07)` and
Cargo 1.97.0. The frozen comparison point is the Phase 0 baseline at commit
`09de1fef3af12844d2346df6bd00bc1ba2d29c2c`; its full inventory is in
[`../hardening-baseline/README.md`](../hardening-baseline/README.md).

## Executive comparison

| Measurement | Phase 0 baseline | Final observation | Change |
| --- | ---: | ---: | ---: |
| Lexical `///` lines under crate `src/` trees | 169 | 1,568 | +1,399 |
| Lexical public declaration lines | 527 | 469 | -58 |
| Production `.unwrap()` call sites | 18 | 0 | -18 |
| Production `.expect()` call sites | 32 | 50 | +18 |
| Production `unwrap` / `expect` call sites in total | 50 | 50 | unchanged |
| Product integration-test binaries | 33 | 14 | -19 (-57.58%) |
| Tests in those product integration binaries | 98 | 98 | unchanged |
| Workspace tests discovered | 544 | 550 | +6 |
| Cold workspace build | 47.95 s wall, 164.77 s user, 32.62 s sys | 42.32 s wall, 159.96 s user, 31.99 s sys | -11.7% wall |
| Cold workspace test | 99.09 s wall, 246.97 s user, 48.88 s sys | 77.53 s wall, 235.06 s user, 45.09 s sys | -21.8% wall |

The final timing run used the same isolated, reduced-priority convention as
Phase 0. Compiler and registry caches remained warm, while each command used
its own initially empty target directory:

```sh
final=$(mktemp -d /tmp/rsproxy-hardening-final.XXXXXX)
rsync -a --exclude .git --exclude target ./ "$final/"
cd "$final"
/usr/bin/time -p env CARGO_TARGET_DIR="$final/target-build" nice -n 10 \
  cargo build --workspace --timings --quiet
/usr/bin/time -p env CARGO_TARGET_DIR="$final/target-test" nice -n 10 \
  cargo test --workspace --no-fail-fast --quiet
```

The final workspace test run discovered 550 tests: 549 passed and the explicit
1 GiB proxy-stream resource test remained ignored as designed.

## Rustdoc surface indicator

These are the same lexical indicators used by Phase 0, not a semantic coverage
percentage. "Doc lines" matches `^[[:space:]]*///`; "public declarations"
matches an unqualified `pub` followed by `fn`, `struct`, `enum`, `trait`,
`type`, `const`, `static`, `mod`, or `use`. It intentionally excludes crate
docs, public fields, and enum variants. The authoritative final gates are
workspace `missing_docs = "deny"` and rustdoc with warnings denied.

| Crate | Baseline `///` | Final `///` | Baseline public declarations | Final public declarations |
| --- | ---: | ---: | ---: | ---: |
| rsproxy-rules | 6 | 443 | 103 | 103 |
| rsproxy-trace | 0 | 244 | 58 | 58 |
| rsproxy-net | 32 | 234 | 114 | 112 |
| rsproxy-platform | 19 | 215 | 69 | 69 |
| rsproxy-engine | 31 | 206 | 57 | 57 |
| rsproxy-control | 32 | 96 | 24 | 24 |
| rsproxy-cli | 38 | 118 | 74 | 17 |
| xtask | 11 | 12 | 28 | 29 |
| **Workspace** | **169** | **1,568** | **527** | **469** |

Reproduce the final lexical counts from the repository root:

```sh
for crate in rsproxy-rules rsproxy-trace rsproxy-net rsproxy-platform \
  rsproxy-engine rsproxy-control rsproxy-cli xtask
do
  doc=$(rg -n '^[[:space:]]*///' "crates/$crate/src" -g '*.rs' | wc -l)
  public=$(rg -n \
    '^[[:space:]]*pub[[:space:]]+((async|unsafe|const)[[:space:]]+)*(fn|struct|enum|trait|type|const|static|mod|use)([[:space:]]|$)' \
    "crates/$crate/src" -g '*.rs' | wc -l)
  printf '%s\t%s\t%s\n' "$crate" "$doc" "$public"
done
```

## Production `unwrap` / `expect` audit

The product-code inventory remains exactly 50 call sites, preserving the panic
boundaries measured in Phase 0. All 18 baseline `unwrap` calls are now explicit
`expect` calls with messages that state the lock, construction, serialization,
or already-validated invariant. Workspace Clippy denies any new `unwrap`.

| Crate | Baseline `unwrap` | Baseline `expect` | Final `unwrap` | Final `expect` |
| --- | ---: | ---: | ---: | ---: |
| rsproxy-control | 0 | 3 | 0 | 3 |
| rsproxy-engine | 8 | 13 | 0 | 21 |
| rsproxy-net | 6 | 10 | 0 | 16 |
| rsproxy-platform | 0 | 1 | 0 | 1 |
| rsproxy-rules | 3 | 1 | 0 | 4 |
| rsproxy-trace | 1 | 4 | 0 | 5 |
| rsproxy-cli | 0 | 0 | 0 | 0 |
| **Product crates** | **18** | **32** | **0** | **50** |

The path exclusions deliberately match the Phase 0 audit: directories or files
named `tests`, `tests.rs`, `test_support`, or `test_support.rs` are omitted.
List every final call site and verify the method split with:

```sh
rg -n '\.(unwrap|expect)\(' crates/rsproxy-*/src \
  -g '*.rs' \
  -g '!**/tests/**' -g '!**/tests.rs' \
  -g '!**/test_support/**' -g '!**/test_support.rs'

rg -o --no-filename '\.(unwrap|expect)\(' crates/rsproxy-*/src \
  -g '*.rs' \
  -g '!**/tests/**' -g '!**/tests.rs' \
  -g '!**/test_support/**' -g '!**/test_support.rs' | sort | uniq -c
```

The second command prints only `50 .expect(`; a dedicated `.unwrap(` search
returns no matches.

## Product integration-test binaries

Cargo metadata reports 14 integration targets across the seven product
packages. Harness `--list` reports 98 tests, exactly preserving the Phase 0
black-box test inventory while eliminating 19 linker units.

| Crate | Binary | Tests |
| --- | --- | ---: |
| rsproxy-rules | it | 13 |
| rsproxy-rules | public_api | 2 |
| rsproxy-trace | public_api | 3 |
| rsproxy-net | it | 15 |
| rsproxy-net | public_api | 7 |
| rsproxy-engine | it | 10 |
| rsproxy-engine | public_api | 5 |
| rsproxy-control | public_api | 3 |
| rsproxy-platform | it | 10 |
| rsproxy-platform | public_api | 5 |
| rsproxy-cli | cli_daemon_lifecycle | 5 |
| rsproxy-cli | cli_product_matrix | 4 |
| rsproxy-cli | it | 15 |
| rsproxy-cli | large_stream_resource | 1 |
| **Total** | **14 binaries** | **98** |

Reproduce the target inventory and Cargo harness counts without executing the
tests:

```sh
cargo metadata --no-deps --format-version 1 |
  jq -r '
    .packages[]
    | select(.name | startswith("rsproxy-"))
    | .name as $package
    | .targets[]
    | select(.kind == ["test"])
    | [$package, .name]
    | @tsv
  ' |
  while IFS="$(printf '\t')" read -r package target
  do
    count=$(cargo test -q -p "$package" --test "$target" -- \
      --list --format terse | awk '/: test$/ { count += 1 } END { print count + 0 }')
    printf '%s\t%s\t%s\n' "$package" "$target" "$count"
  done
```

The separate `xtask/tests/public_api.rs` target contains two additional tests
and is intentionally excluded from the product-binary comparison.

## Final hardening gates

The following commands were run from the working tree on 2026-07-13. They are
the local hardening subset of the cross-platform CI contract.

| Gate | Command | Final observation |
| --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | PASS |
| Curated lint policy | `cargo clippy --workspace --all-targets --locked -- -D warnings` | PASS, zero warnings |
| Rustdoc and links | `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked` | PASS, all 8 workspace packages documented |
| Tests | `cargo test --workspace --all-targets --no-fail-fast --locked` | PASS, 550 discovered / 549 passed / 1 ignored |
| Repository contracts | `cargo xtask check all` | PASS: 7 API snapshots, lines, layout, typed errors, and workflows |

The structural acceptance checks also pass:

```sh
find crates -type d -empty -print
rg -n 'pub use .*::\*' crates/*/src/lib.rs
rg -n 'impl([[:space:]]+[^[:space:]]+)*[[:space:]]+Deref(Mut)?' \
  crates/rsproxy-cli/src
rg -n '\.unwrap\(' crates/rsproxy-*/src \
  -g '*.rs' \
  -g '!**/tests/**' -g '!**/tests.rs' \
  -g '!**/test_support/**' -g '!**/test_support.rs'
```

All four commands produce no matches. The API gate uses
`cargo-public-api 0.52.0` with the pinned `nightly-2026-07-10` toolchain; the
product build and all other gates remain on stable Rust.
