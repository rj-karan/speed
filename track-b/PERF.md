# Track B: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 56.35 | 2,200 | 0 |
| 1 | Query-first posting lists in C++, plus candidate verification for phrases | run-e6f4196a5413 | 13.192232 | 135.73 | 24.62 |
| 2 | Index documents and answer independent queries with six workers each | run-c7f49f1065ff | 2.118497 | 245.15 | 41.28 |
| 3 | Replace per-token unordered-map lookups with a compact collision-checked table | run-ca5488ac5c2e | 1.597651 | 249.21 | 43.73 |
| 4 | Verify phrase matches with an eight-token ring buffer instead of allocating a vector per candidate | run-15fdbe77791b | 1.451663 | 249.40 | 44.77 |
| 5 | Move and release worker posting segments as they are merged | run-7188c2f6ba77 | 1.476454 | 249.84 | 44.54 |
| 6 | Revert the merge change after it failed to lower peak memory | run-394147bf29ed | 1.436055 | 249.6 | 44.88 |
| 7 | Scale indexing and query evaluation to all board threads, and match phrase tokens against the query's own terms | run-6b150988b5e8 | 1.017699 | 282.8 | 47.73 |

Local `check.py` passed the official seed-1 full output hash
`f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927`.
The local Docker check took 11.30 s including container startup; this is not a
board measurement and is not used as a judging claim.

The seed-2 sample generated from the published generator also matched its Python
reference output hash `86b92e3ceda664396bad5cabc3809b447de0610282eee484605fd23cbd12fea6`.

Parallel indexing alone reduced the full local sandbox run to 9.45 s. Parallel
query evaluation brought it to 3.56 s, with both the full public hash and the
seed-2 sample hash unchanged. The board measured 2.118497 s and 245.15 MB;
despite the memory increase, K rose from 3.27 to 6.69.

The compact lookup raised K from 6.69 to 7.49 on the board. Hash matches are
checked against the original query bytes, so collisions cannot alter answers.
The ring-buffer check raised K from 7.49 to 7.86 on the board.
The move-and-release merge made K worse, 7.90 to 7.78, and did not lower the
kernel's peak memory counter; the allocator retained its pages. It was reverted
to K 7.90 (run-394147bf29ed).

Two further changes (commits d14f441 then 7dfa3a9) were measured together on the
board as run-6b150988b5e8. First, indexing and query evaluation were raised from
six workers to hardware_concurrency: indexing tokenizes the full 253 MB corpus
and is the largest phase, and the board offers twelve threads. Second, a profile
showed phrase queries are 38% of the set but about 78% of query CPU, because the
phrase verifier re-tokenizes each candidate document and probed the ~150k-entry
term table for every token. Matching each document token against only that
phrase's (at most eight) terms keeps the inner loop cache-resident; locally that
cut phrase CPU from 3697 ms to 1740 ms and query wall time from 520 ms to 346 ms.
Tokenization rules and the adjacency window are unchanged, so the output is
identical. On the board the combined result was 1.017699 s at 282.8 MB, raising K
from 7.90 to 9.01. Peak memory rose slightly with the extra per-worker posting
headers but stays far under the 2.2 GB reference.
