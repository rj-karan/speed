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

## Anchored phrase verification and sparse Boolean operations (October 31)

Board confirmation: `run-5be15848bfb6` at commit `f6f0c74` passed every gate.
It measured 0.749897 s, 270,413,824 bytes, S=429, K=10.447687 and 50.951008
machine points. Against the previous best, time fell 24.98%, peak memory fell
4.81%, and K rose 15.08% despite the source increase. No local timing below is
claimed as an official score. The previous best public run is `run-e061987e4a0a` at
`abc2c95`: 0.999600 s, 284,090,368 bytes, S=393, K=9.078875, 47.901602 points.

Replace the per-token phrase hash/window loop with an AVX2 first-byte search for
the rarest term, exact term/boundary validation, and forward/backward neighbour
checks. No positional index or candidate allocation is needed. Sparse Boolean
intersections/differences use advancing lower_bound searches when the list-size
ratio exceeds 32; comparable lists retain linear merges. Cap workers at twelve
instead of sixteen to match the available hardware threads and reduce duplicate
posting headers and allocator overhead.

Local five-run A/B test on an AMD Ryzen 7 7435HS, pinned to six physical cores and
their SMT siblings (`0-5,8-13`), prewarmed public input, 8 GB/no swap, read-only
root, no network, unprivileged submission. The official `sd-timer.c` wrapper
measured process start to exit, excluding Docker startup. Runs alternated old and
new; all ten matched the official full hash. Discarding each version's highest
and lowest yielded:

| Version | Five process times (s) | Middle-three mean (s) |
|---|---|---|
| Before | 0.832480, 0.819787, 0.823531, 0.816122, 0.797028 | 0.819813 |
| After | 0.685327, 0.656984, 0.668693, 0.650092, 0.662451 | 0.662709 |

That is a 19.17% local reduction, not a prediction of the scoring CPU's gain.
The phrase-only experiment initially measured 0.795288 → 0.707241 s. Local
benchmarks do not measure the official cgroup peak or formatted source penalty;
the official run above confirms that the combined coefficient improved.

Correctness: full seed-1 output remains
`f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927`;
official seed-2 sample remains
`86b92e3ceda664396bad5cabc3809b447de0610282eee484605fd23cbd12fea6`;
official seed-17 tiny matches its reference
`e67d694cae7b8a126e3931c7dbc74cdf91d655fcf0eafb31da3fc02546166079`.
A separate byte-regex oracle checks 2,500 mixed queries, repeated terms, absent
terms, token prefixes, 32-byte terms, uppercase, punctuation and non-ASCII bytes
with the actual sandboxed image (`python3 -m unittest discover -s tests -v`).

## Exact hybrid storage (official board confirmation pending)

Replace per-worker growing posting vectors and serial merging with two corpus
passes: count distinct document occurrences, allocate exact sparse postings or
bitmaps by the smaller byte cost, then fill disjoint worker ranges. Boolean
scratch uses word operations for common terms and sparse updates for rare terms.
A bounded SIMD tokenizer and packed hash remove per-character hash multiplication;
all hash matches retain exact byte validation.

Five alternating local process runs, prewarmed seed-1 full data, six physical
Ryzen cores plus SMT and the official timer: old middle-three mean 0.649946 s;
new 0.586214 s (9.8% reduction). Local peak cgroup memory with a shell wrapper
was 92,078,080 bytes versus about 270 MB previously. These are local results,
not official scores. Source-size cost still needs board confirmation.

Public full hash, official seed-2 sample, official seed-17 tiny reference, and the
2,500-query independent byte-regex oracle all pass.

## Block tokenization and batched lookup (board confirmation pending)

Official baseline `run-1f45f9a9e867` at `1a14f38`: 1.003784 s, 91,025,408 bytes,
S=449, K=11.150792 and 52.365287 points. Memory improved, but the two-pass
index was slower on the scoring CPU than our previous posting-vector version.

A one-pass adaptive-vector/bitmap experiment passed correctness but raised the
local peak to about 202 MB for a 13% time saving. Rejected: the combined memory
and source penalty did not justify it. A direct short-word lookup alone saved
about 6% locally, also insufficient as the main change.

The submitted follow-up classifies 32-byte blocks and emits token boundaries
from a transition mask, carrying tokens across blocks. This replaces one SIMD
classification per token with one per block. Sixteen prepared tokens batch hash
probes and prefetch their slots. A collision-free 20-bit key handles one-to-four
ASCII letters; digit-containing and longer tokens use exact collision-checked
hash lookup. Phrase answers collect count/sum/first ten directly from sorted
candidates, and posting membership checks reject impossible candidates before
reading their document text. No full-size result bitmap is needed for phrases.

Five alternating local full-data process runs: original compact index middle-three
mean 0.692636 s; new 0.497411 s (28.2% reduction). These are local numbers, not a
prediction of the official score. Full public hash, seed-2 sample, seed-17 tiny,
and the independent 2,500-query oracle pass. Official K remains pending.
