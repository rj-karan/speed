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
