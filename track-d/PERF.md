# Track D: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 72.69 | 893 | 0 |
| 1 | Use forward and reverse CSR for bidirectional 64-bit Dijkstra with stamped distances | run-f48fbd5429aa | 5.264194 | 137.60 | 32.31 |
| 2 | Process independent queries on six workers with one search state per worker | run-c2344a47e783 | 1.707945 | 348.94 | 40.07 |
| 3 | Parallel 4-thread CSR graph build, 4-ary QuadHeap, 16-bit generation stamps, 8 workers | run-f214f61af425 | 1.405876 | 413.58 | 39.74 |
| 4 | Reduce query workers to 4 to cut peak memory below 300 MB while preserving 1.4s graph build and query throughput | run-b8eec44801f0 | 1.544531 | 271.41 | 40.54 |

Local `check.py` passed the official seed-1 full output hash
`a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e`.
The local Docker check took 8.24 s including container startup; this is not a
board measurement and is not used as a judging claim.

Second local experiment: build reverse CSR and search from both ends. The full
local sandbox check fell to 7.13 s and kept the same output hash. The board will
measure whether the extra reverse graph memory is worth the time saved.

The seed-2 sample generated from the published generator also matched its Python
reference output hash `70e94813fc6fbee12c22cc040bc4a4629c70508d13eaab2bfabbdd75e1d51aed`.

Six-worker local sandbox run on the full public graph took 3.65 s (previously
7.13 s) and matched the same official hash. The official board run will measure
the speed and peak-memory trade-off.
The board confirmed 1.707945 s and 348.94 MB for run-c2344a47e783. The extra
memory cost was outweighed by the faster run: K rose from 4.43 to 6.33.

## Radix heap and CSR sentinel fix (board confirmation pending)

Profiling the old implementation locally found graph construction around 311 ms
and query evaluation around 1,499 ms. Replace the comparison heap with a monotone
65-bucket radix heap. Keep four workers and full-node generation-stamped distance
arrays. Correct the CSR cursor sentinel loop's out-of-bounds read (u < n).
Path sums remain uint64 and results retain signed -1 for unreachable.

Five alternating local full-data process runs measured previous middle-three mean
2.110186 s and radix-only 1.401386 s. Background-machine variance was substantial:
previous runs ranged 1.878–2.278 s and new runs 1.255–1.558 s. The ranges did not
overlap, but the board must confirm the actual gain; these are not official scores.

An eight-worker sparse-state experiment also passed correctness and measured
1.850205 → 1.037898 s in an earlier local A/B batch. It was rejected before
submission: its cgroup peak was 545,898,496 bytes, roughly twice the previous
peak, so the time win did not justify memory and extra source-size costs.

The submitted radix-only version passes the full public hash, official seed-17
tiny reference, and 1,150 independent heapq-oracle queries, including duplicate,
parallel and zero-weight edges, isolated nodes and sums exceeding uint32.
Official run ID and K remain pending.

Official confirmation for the radix-only version: `run-a361068428ee` at
`1a14f38` measured 1.144292 s, 277,512,192 bytes, S=312, K=7.479621 and
43.693980 points, passing all gates.

A further on-the-fly bidirectional degree-two chain shortcut experiment passed
the public hash, seed-17 tiny and independent heapq oracle. It was rejected:
five alternating local runs gave 1.412830 s before and 1.568383 s after. The
extra neighbourhood checks cost more than the heap operations they removed.
The submitted radix-only implementation remains unchanged.

An offline native/PGO build trained on newly generated seed-17 sample data passed
all five full-data hashes. The alternating local comparison measured 1.484468 s
before and 1.304052 s after. This candidate is retained outside the submission
for further evaluation; it was not promoted because D remains well below the
three counted tracks and this round prioritizes the overall title. No official
PGO gain is claimed. The latest radix-only confirmation, `run-47c645bff20d` at
`af43e3f`, passed with 1.047887 s, 279,580,672 bytes, S=312, K=7.804513 and
44.617289 points. This varies from the earlier run of unchanged code and is not
attributed to another algorithm change.

## Research-led layout and build redesign (board confirmation pending)

Research: the authors' PHAST report discusses vertex numbering and cache
locality: https://www.microsoft.com/en-us/research/wp-content/uploads/2010/09/phastTR.pdf
Our implementation does not use their software or a routing library. It tests
simple runtime numbering and includes all reordering cost in the measured run.

Five-run alternating local full-data process comparisons:

| Candidate | Baseline middle-three mean | Candidate middle-three mean |
|---|---:|---:|
| DFS numbering | 1.260434 s | 0.906840 s |
| BFS numbering | 1.256420 s | 0.828045 s |
| BFS + packed distance/stamp | 1.265630 s | 0.836891 s |
| Packed parsing + reuse degree buffers | 1.263879 s | 0.804635 s |
| Shared counters, 12 build workers, BFS, packed state/parser | 1.270298 s | 0.650235 s |

All runs matched the full public hash. The selected final candidate reduced local
process time 48.8% against the submitted baseline, including graph build,
renumbering, searches and output. Its local shell-wrapped cgroup peak was
229,085,184 bytes. Packed state was retained for its lower memory footprint,
not a claim that it independently improved runtime.

The selected implementation uses 53 distance bits and eleven generation bits;
n*UINT32_MAX < 2^53 on all published scales. A 10,000-query independent fixture
forces wraparound and checks two UINT32_MAX edges sum to 8,589,934,590. Official
seed-7 sample matches `88bf36c94620505d1beafe7cd56a08dc2d261b71f9824d2f764d5ce6e5d7872e`;
seed-17 tiny and the independent random-graph oracle also match.

Native/PGO training on the new candidate was tested and rejected: 0.648752 ->
0.647854 s locally was indistinguishable from noise. The training generator,
profiles, generated training data and rejected candidates are not submitted.
Official run ID, peak and K are pending; local timings are not official scores.

Official confirmation `run-70a2baae990f` at `934d875` passed every gate:
0.668657 s, 229,572,608 bytes, S=345, K=10.010806 and 50.023453 points. Against
`af43e3f` / `run-47c645bff20d`, time fell 36.2% and memory 17.9%; points rose
from 44.617289 to 50.023453 despite source increasing from 312 to 345 lines.
This confirms the layout/build/state change improved the official coefficient.
D still remains below the three counted tracks; this improvement alone does
not change the overall title score.
