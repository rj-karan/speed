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

## Hour-local C++ redesign (October 31; run-23861fe20fc1)

Focus is Track A only; other track implementations are unchanged. Rejected local
prototypes included a two-pass dictionary/cursor rewrite (slower than the existing
Rust program) and a whole-input packed-record variant (higher peak storage).
The selected design indexes contiguous runs by hour, then lets each worker own
an entire hour's parsing, grouping, quantiles and formatting. This removes global
per-request atomic counters and scatter cursors and keeps temporary latency arrays
small. All hour runs are discovered at runtime; out-of-order hours remain correct.

Initial x86-64-v3 local A/B timings (Ryzen 7 7435HS, six physical cores plus SMT,
prewarmed input, official process timer, five alternating runs): Rust middle-three
mean 0.045164 s; hour-local C++ 0.037621 s. Local peak cgroup memory including a
shell wrapper was 35,266,560 bytes. These are local experiments, not official
board measurements. The submitted build uses native CPU tuning, which the rules
allow and which triggers compilation on the scoring machine itself.

Full public output hash and seed-17 sample reference match. An independent oracle
also verifies 6,000 shuffled requests with injected duplicates, all methods,
32-bit latency extremes, every decimal length, month/hour boundaries, prefix
ordering and endpoint lengths at vector boundaries; empty input is checked too.
The requested 85-point target has not been established by a board run yet.

## Release output buffers as they are written (run-26990cc808b6)

The hour-local redesign at `d9db9d0` reached 72.787 machine points on the board
(run-23861fe20fc1). The next iteration changes
only output storage: bounded, anonymous mapped buffers use the specification's
maximum row length instead of allocator-owned strings. Each buffer is unmapped
immediately after its ordered write. This avoids retaining private output pages
while the kernel accumulates the output file's page cache.

Local peak cgroup memory fell from about 35.3 MB to 24.3 MB. Process runtime stayed
around 0.038 s on this Ryzen, so the expected benefit is memory, not a speed claim.
Full public hash, official seed-17 sample, empty input and the shuffled edge-case
oracle all pass. Batched endpoint prefetch and MAP_POPULATE experiments were
rejected after they failed to improve local timings. The 85-point target remains
unmet; other tracks now need work as well.

Official confirmation for `c4baf88`: `run-26990cc808b6` passed every gate,
measuring 0.047678 s, 23,441,408 bytes, S=323, K=31.417257 and 74.858413
points. This improves on `d9db9d0` / `run-23861fe20fc1` at 72.786677 points.

PGO experiment rejected: an offline GCC build trained on the byte-identical
published generator's seed-17 sample. Five alternating full-data runs gave
0.036724 s without PGO and 0.036759 s with it; every output hash matched. No
measurable gain justified changing the build. The experimental binary and
training data are not part of the submitted runtime image.
