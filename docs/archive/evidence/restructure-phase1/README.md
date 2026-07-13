# Phase 1 governance evidence

This same-machine comparison was captured on 2026-07-12 after enabling thin
LTO, one codegen unit, and symbol stripping in the workspace release profile.

- Release binary: 10,982,864 bytes, down 29.36% from the pre-restructure
  15,548,144-byte binary.
- H1 proxy throughput: 34,176.78 requests/second at 10,000 requests and
  concurrency 32, down 7.36% from the same-machine 36,891.38 requests/second
  baseline and therefore inside the allowed 10% band.
- Status and I/O errors: zero.
- Release binary SHA-256:
  `84f7ed4f2172be57f42b11eac6e427677ee2d23f08a0576bb5a3033d67bcd2fb`.

`h1.json` contains the complete benchmark result. The Phase 0 Criterion report
remains the immutable comparison source for later crate extraction phases.
