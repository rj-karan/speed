# Track A: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 32.05 | 228 | 0 |
| 1 | Replace per-line Python date parsing with fixed-offset C++ parsing; intern endpoints and sort latency vectors only at output | run-28310cdd1a9c | 0.510158 | 35.21 | 50.10 |
| 2 | Pure Rust rewrite (D=1): 12-thread parallel chunk parsing, open-addressing endpoint interning, global radix bucket scatter and parallel sort | pending board ID | pending | pending | pending |
| 3 | Lock-free atomic endpoint interning with thread-local cache, fused FNV-1a hash traversal, zero-reduction global atomic accumulators, parallel atomic scatter, and parallel multi-threaded formatting across 72 hour slots | pending board ID | 0.0745 | 57.5 | 65.24 |
| 4 | Persistent 12-worker thread pool with Barrier synchronization, 64-bit SWAR endpoint scanning, branchless method jumping, MAP_SHARED page-table bypass, and exact post-parse latency buffer allocation | pending board ID | pending | pending | pending |

Local `check.py` passed the official seed-1 full output hash
`495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3`.
The local Docker check took 0.27 s including container startup; this is not a
board measurement and is not used as a judging claim.
