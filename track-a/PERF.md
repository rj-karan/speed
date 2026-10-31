# Track A: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 32.05 | 228 | 0 |
| 1 | Replace per-line Python date parsing with fixed-offset C++ parsing; intern endpoints and sort latency vectors only at output | run-28310cdd1a9c | 0.510158 | 35.21 | 50.10 |
| 2 | Pure Rust rewrite (D=1): 12-thread parallel chunk parsing, open-addressing endpoint interning, global radix bucket scatter and parallel sort | run-bdc0f2095f6c | 0.0745 | 57.5 | 65.24 |
| 3 | Lock-free atomic endpoint interning with thread-local cache, fused FNV-1a hash traversal, zero-reduction global atomic accumulators, parallel atomic scatter, and parallel multi-threaded formatting across 72 hour slots | run-bdc0f2095f6c | 0.0745 | 57.5 | 65.24 |
| 4 | Persistent 12-worker thread pool with Barrier synchronization, 64-bit SWAR endpoint scanning, branchless method jumping, MAP_SHARED page-table bypass | run-7ba00dc18712 | 0.0544 | 50.4 | 69.06 |
| 5 | Two-pass streaming scan completely eliminating items Vec<u64> allocations (saving >21 MB RAM), fused cache-resident group percentile extraction & output formatting | run-7912c18c175d | 0.0620 | 29.3 | 69.72 |
| 6 | SWAR newline skipping in Pass 1, read-only zero-CAS endpoint lookup in Pass 2, and direct raw pointer summary formatting | run-f056314f934c | 0.0603 | 29.1 | 69.71 |
| 7 | Thread-local non-atomic count/error arrays in Pass 1, parallel slot reduction, zero-alloc set_len for latency storage, lock-free thread-local range cursors in Pass 2 eliminating all atomic contention, L1 fast endpoint table lookup, and sorting network for N <= 16 | run-e9b9677c692d | 0.063711 | 30.95 | 68.04 |

Local `check.py` passed the official seed-1 full output hash
`495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3`.
The local Docker check took 0.24 s including container startup; this is not a
board measurement and is not used as a judging claim.

## Rejected experiment and restoration (October 31)

Step 7 was measured at commit `f09e076` as `run-e9b9677c692d`: 0.063711 s,
30,953,472 bytes, S=689, K=22.946816 and 68.036121 machine points. The earlier
`060f342` implementation (`run-7912c18c175d`) used 0.061999 s, 29,294,592 bytes
and S=484, producing K=24.798634 and 69.721388 points. The extra per-worker
arrays and cursor machinery did not pay for their memory and source-size cost.
Restored `src/main.rs` byte-for-byte from `060f342`; its Dockerfile and manifest
are unchanged. The confirmation run `run-86226c1c0649` at `f6f0c74` passed every gate:
0.062557 s, 29,454,336 bytes, S=484, K=24.660949 and 69.600489 points.
This confirms the restored implementation beats the rejected cursor experiment. Full public hash and a
seed-17 sample comparison against the official reference verify the restoration.

Steps 2 and 3 cite the same run: they are not independent measured improvements.
Only their combined measured state is supported by that run ID.
