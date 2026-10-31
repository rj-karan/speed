# Track A: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change/status | Commit | Run ID | T (s) | M (MB, decimal) | S | K | Points |
|---|---|---|---|---|---|---|---|---|
| 0 | Reference (published rounded axes) | — | — | 32.05 | 228 | — | 1 | 0 |
| 1 | Replace per-line Python date parsing with fixed-offset C++ parsing; intern endpoints and sort latency vectors only at output | `d31be19` | run-28310cdd1a9c | 0.510158 | 35.213 | 139 | 10.047510 | 50.102922 |
| 2 | Pure Rust rewrite (D=1): 12-thread parallel chunk parsing, open-addressing endpoint interning, global radix bucket scatter and parallel sort | `4239bbb` | run-bdc0f2095f6c | 0.074484 | 57.545 | 423 | 20.170691 | 65.236039 |
| 3 | Same measured state as preceding row; no separate improvement | `4239bbb` | run-bdc0f2095f6c | 0.074484 | 57.545 | 423 | 20.170691 | 65.236039 |
| 4 | Persistent 12-worker thread pool with Barrier synchronization, 64-bit SWAR endpoint scanning, branchless method jumping, MAP_SHARED page-table bypass | `27c2ee2` | run-7ba00dc18712 | 0.054424 | 50.438 | 443 | 24.060064 | 69.064839 |
| 5 | Two-pass streaming scan completely eliminating items Vec<u64> allocations (saving >21 MB RAM), fused cache-resident group percentile extraction & output formatting | `060f342` | run-7912c18c175d | 0.061999 | 29.295 | 484 | 24.798634 | 69.721388 |
| 6 | Rejected slight score regression: SWAR newline skipping in Pass 1, read-only zero-CAS endpoint lookup in Pass 2, and direct raw pointer summary formatting | `abc2c95` | run-f056314f934c | 0.060327 | 29.168 | 535 | 24.786504 | 69.710764 |
| 7 | Rejected regression: thread-local non-atomic count/error arrays in Pass 1, parallel slot reduction, zero-alloc set_len for latency storage, lock-free thread-local range cursors in Pass 2 eliminating all atomic contention, L1 fast endpoint table lookup, and sorting network for N <= 16 | `f09e076` | run-e9b9677c692d | 0.063711 | 30.953 | 689 | 22.946816 | 68.036121 |
| 8 | Restored Rust after cursor regression | `f6f0c74` | run-86226c1c0649 | 0.062557 | 29.454 | 484 | 24.660949 | 69.600489 |
| 9 | Hour-local C++ redesign | `d9db9d0` | run-23861fe20fc1 | 0.050142 | 34.177 | 312 | 28.558378 | 72.786677 |
| 10 | Release mapped output buffers | `c4baf88` | run-26990cc808b6 | 0.047678 | 23.441 | 323 | 31.417257 | 74.858413 |
| 11 | Unchanged-source confirmation | `af43e3f` | run-0ef7bf129650 | 0.046833 | 23.835 | 323 | 31.594127 | 74.980318 |
| 12 | Rejected single-pass memory/source trade-off | `934d875` | run-79c5144df686 | 0.044225 | 26.599 | 345 | 31.493662 | 74.911158 |
| 13 | Restore higher-K hour-local source | `1311354` | run-7a33af9abbe9 | 0.046758 | 23.802 | 323 | 31.628157 | 75.003694 |
| 14 | Unchanged-source confirmation | `9c8f98c` | run-24951b257cd0 | 0.046647 | 23.544 | 323 | 31.734877 | 75.076841 |
| Follow-up | Rejected static/calendar trial; restoring previous build and arithmetic | `a3babda` | run-e30fa8549d30 | 0.051866 | 23.163 | 325 | 30.166297 | 73.976100 |

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

Steps 2–7 were Rust implementations. The later hour-local redesign switched
back to C++ after the local comparison below, and the submitted program is C++.
The language label in step 2 describes that historical version.

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

## Rejected single-pass record experiment (official trial below)

Remove the full-input hour-run discovery scan. Workers parse byte ranges once
into private hour-specific six-byte records, then merge only endpoint catalogs.
Mapped record buffers grow when necessary and are released as each hour is
consumed. This avoids the allocator-retention cost of the earlier rejected
whole-input vector experiment. Output remains explicitly mapped and reclaimed.

Five alternating full-data process runs on six Ryzen cores plus SMT measured
0.035522 s before and 0.030745 s after (13.4% reduction). One local peak check
measured 26,808,320 bytes; this trades some memory for time, so official K must
confirm the choice. All output hashes matched. Full public input, the official
seed-17 sample, and independent oracles including 800,000 duplicate lines (forcing
mapping growth) pass. No score is claimed from these local timings.

A compact private endpoint-byte pool was also tried: it passed correctness but
slowed 0.031074 -> 0.032108 s locally, so it was rejected. The extra PGO build
again showed no measurable benefit on the selected code.

Official trial `run-79c5144df686` at `934d875` measured 0.044225 s,
26,599,424 bytes, S=345, K=31.493662 and 74.911158 points. The earlier hour-local
version at `af43e3f` (`run-0ef7bf129650`) measured 0.046833 s, 23,834,624 bytes,
S=323, K=31.594127 and 74.980318 points. The local time gain did not transfer
fully to the scoring CPU, and the extra memory/source cost erased it. The
single-pass mapped-record implementation was therefore reverted to the exact
`af43e3f` source. Both are correct; the measured combined coefficient drives
this choice. The new large-group oracle remains useful for either implementation.

## Static-link and readability follow-up

Five local interleaved full-data process runs: existing mean 0.037528 s;
static-linked mean 0.036142 s. Separate shell-wrapped peaks were 24,297,472 and
23,994,368 bytes. This small local gain is a candidate, not an official score.
The submitted follow-up retains native tuning, names the fixed calendar window
from spec section 2.1, improves allocation diagnostics, and applies LLVM format.
The public full hash and independent hour/quantile oracle pass.


Official follow-up at `a3babda`: all gates passed. Rejected static/calendar trial; restoring previous build and arithmetic.
The local static-link result did not transfer to the scoring host: 73.976100 points versus the prior 75.076841. The static build is reverted, and the original direct hour-offset calculation is expressed with named constants. No score gain is attributed to readability changes.
