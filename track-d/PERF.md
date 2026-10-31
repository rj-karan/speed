# Track D: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 72.69 | 893 | 0 |
| 1 | Use forward and reverse CSR for bidirectional 64-bit Dijkstra with stamped distances | run-f48fbd5429aa | 5.264194 | 137.60 | 32.31 |
| 2 | Process independent queries on six workers with one search state per worker | run-c2344a47e783 | 1.707945 | 348.94 | 40.07 |
| 3 | Pure Rust rewrite (D=1): One-pass text streaming + binary scatter, 4-ary QuadHeap Dijkstra with branch pruning, 12-thread work-stealing | pending board ID | pending | pending | pending |

Local `check.py` passed the official seed-1 full output hash
`a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e`.
The local Docker check took 1.78 s including container startup; this is not a
board measurement and is not used as a judging claim.
