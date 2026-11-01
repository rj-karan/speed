# Track B: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change/status | Commit | Run ID | T (s) | M (MB, decimal) | S | K | Points |
|---|---|---|---|---|---|---|---|---|
| 0 | Reference (published rounded axes) | — | — | 56.35 | 2,200 | — | 1 | 0 |
| 1 | Query-first posting lists in C++, plus candidate verification for phrases | `d31be19` | run-e6f4196a5413 | 13.192232 | 135.733 | 246 | 3.107840 | 24.622935 |
| 2 | Index documents and answer independent queries with six workers each | `35c44e9` | run-c7f49f1065ff | 2.118497 | 245.146 | 299 | 6.691846 | 41.277296 |
| 3 | Replace per-token unordered-map lookups with a compact collision-checked table | `c2a6ab9` | run-ca5488ac5c2e | 1.597651 | 249.213 | 353 | 7.491591 | 43.728703 |
| 4 | Verify phrase matches with an eight-token ring buffer instead of allocating a vector per candidate | `a50ddb9` | run-15fdbe77791b | 1.451663 | 249.401 | 353 | 7.858081 | 44.765824 |
| 5 | Rejected regression: move and release worker posting segments as they are merged | `1e130ee` | run-7188c2f6ba77 | 1.476454 | 249.836 | 357 | 7.775965 | 44.537716 |
| 6 | Revert the merge change after it failed to lower peak memory | `0ebeb13` | run-394147bf29ed | 1.436055 | 249.635 | 353 | 7.899190 | 44.879129 |
| 7 | Scale indexing and query evaluation to all board threads, and match phrase tokens against the query's own terms | `7dfa3a9` | run-6b150988b5e8 | 1.017699 | 282.771 | 393 | 9.006160 | 47.726984 |
| 8 | Previous posting-vector comparison | `abc2c95` | run-e061987e4a0a | 0.999600 | 284.090 | 393 | 9.078875 | 47.901602 |
| 9 | Anchored phrase verification | `f6f0c74` | run-5be15848bfb6 | 0.749897 | 270.414 | 429 | 10.447687 | 50.951008 |
| 10 | Exact hybrid allocation: lower memory, slower time | `1a14f38` | run-1f45f9a9e867 | 1.003784 | 91.025 | 449 | 11.150792 | 52.365287 |
| 11 | Block tokenization and batched lookup | `e51a4bd` | run-3550441bc92b | 0.651392 | 96.125 | 530 | 13.355666 | 56.283277 |
| 12 | Unchanged-source confirmation | `1311354` | run-92631bd3cc69 | 0.635049 | 95.687 | 530 | 13.538797 | 56.579004 |
| 13 | Unchanged-source confirmation | `9c8f98c` | run-a8a26830f3fb | 0.646041 | 95.814 | 530 | 13.419566 | 56.386923 |
| Follow-up | Confirmed compact worker-relative postings | `a3babda` | run-f7c72f2f389f | 0.627526 | 71.049 | 546 | 14.390926 | 57.904436 |
| Rejected cached-hit trial | Time improved, combined K did not | `827cf69` | run-d03e8bd3275f | 0.533664 | 100.352 | 600 | 14.359308 | 57.856675 |

| Accepted worker-major cache | All official gates passed | `a9b5f40` | run-2f033ee2f246 | 0.481606 | 92.770 | 639 | 15.210464 | 59.107124 |

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
The move-and-release merge made K worse, 7.86 to 7.78, and did not lower the
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

## Exact hybrid storage (officially confirmed below)

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

## Block tokenization and batched lookup (officially confirmed below)

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
and the independent 2,500-query oracle pass. The official confirmation follows.

Official confirmation: `run-3550441bc92b` at `e51a4bd` passed all gates with
0.651392 s, 96,124,928 bytes, S=530, K=13.355666 and 56.283277 points. Compared
with the previous compact-index run, time fell 35.1%; the measured coefficient
improved despite the additional source and four-megabyte direct-lookup table.

## Further rejected experiments

Offline PGO trained on newly generated seed-17 sample data built successfully,
and all full-data hashes matched. Five alternating local runs gave 0.482199 s
without PGO and 0.479415 s with PGO/native tuning: insufficient evidence of a
meaningful gain. It was not submitted.

A query-directed prototype sampled 1,000 current documents to choose positive
anchors for Boolean disjuncts and phrases, indexing selected anchors and frequent
terms. It verified candidates against actual documents, so sampling never
approximated answers. The full public hash and independent query oracle passed,
but it was rejected: 0.466540 → 1.384542 s locally, with about 182 MB peak.
Candidate verification and temporary posting allocations dominated the saving.

A refinement indexed NOT terms, used bitmap evaluation for fully indexed
expressions and added a 256-bit per-document Bloom rejection filter followed by
exact verification. It again passed the public hash and oracle, but measured
0.438416 → 0.506572 s and 100,560,896 bytes, with more source. Also rejected.
The submitted block-tokenization implementation remains unchanged.

## Worker-relative 16-bit postings (official follow-up requested)

A phase profile of the current full workload measured setup 0.060239 s, counting
0.103437 s, index allocation 0.008433 s, filling 0.170838 s and queries 0.078353 s
locally. Instrumentation printed to stderr in a separate profiling build; no
profiling clock calls are included in the submitted hot path.

A Boolean hybrid vector/bitmap evaluator inspired by sorted-posting intersections
(https://nlp.stanford.edu/IR-book/html/htmledition/processing-boolean-queries-1.html)
passed the hash, but 0.464955 -> 0.453441 s was too small to justify its extra
source. Packed token equality, separate token-start/end masks and a blocked
query-term Bloom filter also failed to improve the combined trade-off reliably.

A sampled one-pass 16-bit-vector prototype avoided the second text scan but
measured 0.456793 -> 0.525476 s and 118,759,424 bytes versus 96,288,768.
Allocation and query-access costs erased its saving. It was rejected. This is
separate from the earlier anchor-directed index experiment, also rejected.

The selected candidate keeps exact two-pass allocation, but stores sparse doc
IDs relative to worker bases in uint16_t. At least four aligned worker ranges
keep spans <= 65,536 at the guaranteed maximum of 250,000 documents; a guard
checks this bound. Per-worker segment ends remain available for exact phrase
membership search and reconstructing global IDs. Dense/sparse selection uses
actual two-byte sparse cost. No token, term or posting is approximated or omitted.

Five interleaved full-data process runs: unchanged baseline middle-three mean
0.476335 s; candidate 0.449513 s (5.6% reduction). All hashes matched. A separate
local shell-wrapped peak check measured about 72.2 MB for the compact index,
versus 96.3 MB. These local figures are not official score claims. A static/native
version was slower (0.484024 s), so the existing x86-64-v3 dynamic build stays.
Full public output and the independent query oracle pass; additional full seeds
are being checked before finalizing the release.


Official follow-up at `a3babda`: all gates passed. Confirmed compact worker-relative postings.
`run-f7c72f2f389f` measured 0.627526 s, 71,049,216 bytes, S=546 and K=14.390926: **57.904436 points**. Compared with unchanged-source `9c8f98c`, memory fell 25.8%, time fell 2.9%, and the combined coefficient improved.


## Flat cached-hit experiments

Several exact single-pass variants avoid re-tokenizing the corpus. Per-worker
32-bit (term, local-doc) hit buffers passed the full hash and oracle, but peaked
at 141,856,768 bytes; sequential scatter into an uninitialized final array and
explicit large-block release reduced that to 117,858,304. In one interleaved
batch, baseline 0.425481 s became 0.360280 s (plain flat hits) or 0.368357 s
(lower peak). Time improved, but the memory/source trade-off remained uncertain.

Replacing each hit's document ID with a per-document cumulative count allowed
three-byte term IDs (106,221,568 bytes) or variable-width packed term IDs
(99,561,472 bytes). The bit-packed version derives width from current queries,
handles words spanning 64-bit boundaries, and retains exact two-byte final
postings. It passes the full hash and independent query oracle. A later
interleaved batch measured baseline 0.414150 s versus 0.349781 s. One upfront
virtual reservation did not establish another gain (0.347036 s, 109,584,384-byte
peak); three-byte reserved buffers measured 0.347661 s and 119,611,392 bytes.
No official point gain is claimed for these prototypes. They remain outside the
submission until the combined coefficient justifies a change.


A separate compact-fingerprint/SIMD dictionary experiment passed the full hash,
but added lookup overhead on the local workload. After reboot, interleaved means
were 0.443517 s for the confirmed compact baseline, 0.482812 s for scalar byte
metadata, 0.532306 s for 16-byte metadata groups, and 0.511552 s for a smaller
SIMD table. These dictionary variants were rejected. Hardware CRC hash variants
also failed to improve the baseline reproducibly (0.422259 -> 0.428605/0.427120 s).
The selected official trial retains the original exact dictionary/hash and uses
one corpus pass plus variable-width cached sparse term IDs. It trades temporary
memory/source for time; only an official K improvement justifies retaining it.


Official cached-hit trial `run-d03e8bd3275f` at `827cf69` passed all gates:
0.533664 s, 100,352,000 bytes, S=600, K=14.359308, 57.856675 points. Runtime
improved 15.0% from the confirmed compact index, but memory rose 41.2% and the
source penalty increased. The combined coefficient was slightly lower than
`run-f7c72f2f389f` (K=14.390926). The exact a3babda source/build is restored.


## Worker-major mapped cache follow-up

A memory phase probe showed why the earlier cached stream had a high peak:
scattering worker zero into term-major output touched almost all 28.6 MB of
sparse output while all 30.4 MB of caches were still live. Worker-major final
segments confine those writes to one worker's contiguous range, after which its
mapped cache is released. Per-worker prefix ends, including empty dense/absent
entries, delimit each sparse term. Sampling chooses only representation, all
terms and occurrences are exact. Bounds and stream width derive from current
input, without cached corpus answers.

The earlier anchor-only prototypes passed correctness but lost the combined
local trade-off: exact-anchor two-pass 0.430443 s versus baseline 0.412453 s;
sampled-anchor one-pass 0.363299 s with a 108,339,200-byte wrapped peak. A pooled
inline-small-list experiment also failed to justify its memory/source cost.
Whole-word equality did not establish a repeatable gain and is excluded.

A clean five-run interleaved comparison gave trimmed means: baseline 0.413235 s,
worker-major mapped cache 0.325041 s, plus dead-allocation trimming/direct dense
first-operand copy 0.313849 s (24.0% below baseline). Separate shell-wrapped peaks
were approximately 72.3 MB baseline, 111.8 MB term-major mapped cache, 84.7 MB
worker-major and 87.2 MB with trim/copy. These local wrapped peaks are not official
memory figures. The previous B source is preserved in git; official K determines
whether this candidate is retained.


The formatted/documented selected build produces the identical runtime image as
the measured trim/copy prototype. It passes the public full hash and all seven
independent oracles. Seeds 19, 37 and 59 each pass twenty full-scale sandbox
runs against the official Python reference. The refreshed root validation record
pins these images and source hashes. This establishes correctness/stability,
not a machine-score improvement; the official trial follows.


Official trial `run-2f033ee2f246` at `a9b5f40` passed every gate and is retained:
0.481606 s, 92,770,304 bytes, S=639, K=15.210464, 59.107124 points. Against
`a3babda` (0.627526 s, 71,049,216 bytes, S=546, K=14.390926), time improves
23.3%, peak increases 30.6%, and combined K improves 5.7%. The net machine gain
is 1.202688 points. This confirms a gain despite the remaining memory/source cost.
The source and output do not depend on the public seed.


## Submitted state

The source/build at `a9b5f40` is retained. Later documentation/tool-only commits
do not change executable source or build settings. These are exact-run metrics, separate from the
board's historical best; normal repeated-run variation is not a new algorithm.

| Source commit | Official run | T (s) | M (bytes) | S | K | Points |
|---|---|---:|---:|---:|---:|---:|
| a9b5f40 | run-2f033ee2f246 | 0.481606 | 92,770,304 | 639 | 15.210464 | 59.107124 |
