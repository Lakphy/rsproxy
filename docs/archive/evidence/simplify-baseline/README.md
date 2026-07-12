# Simplification H1 Decision Evidence

This directory records the short same-machine comparison used for P2.1 on
2026-07-12. Both runs used 10,000 requests at concurrency 32 on the current
Apple M1 Pro macOS host. The established formal baseline remains 45,392 rps;
these short reports only select the implementation direction.

Disabling the handwritten H1 fast path reduced proxy throughput from
36,078.44 rps to 31,254.80 rps, a 13.37% regression. The simplification keeps
the handwritten synchronous H1 path and removes the duplicate Hyper H1 path.
