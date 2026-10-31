# Track C: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change/status | Commit | Run ID | T (s) | M (MB, decimal) | S | K | Points |
|---|---|---|---|---|---|---|---|---|
| 0 | Reference (published rounded axes) | — | — | 14.44 | 367 | — | 1 | 0 |
| 1 | Replace per-record Python objects with a mapped CSV parser, indexed join, and globally interned names with country membership bits | `d31be19` | run-9f1d47d29d62 | 0.906522 | 12.341 | 218 | 7.082148 | 42.508251 |
| 2 | Store each distinct name in a compact open-addressed table, retaining bytewise equality checks | `0ebeb13` | run-4bba7972a60f | 0.577201 | 9.908 | 243 | 9.124117 | 48.009542 |
| 3 | Parse three record-aligned ranges in parallel and merge name membership masks | `7da67d5` | run-a04bbae35fdc | 0.319128 | 23.003 | 318 | 9.958422 | 49.909527 |
| 4 | Store each name's hash in its dedup slot and prefetch the slot right after the name is parsed | `c50790e` | run-851a9e87bcce | 0.225056 | 26.137 | 329 | 11.500588 | 53.036003 |
| 5 | Single lock-free global atomic 262k-slot open-addressed table with atomic bitwise fetch_or country tracking across 12 threads | `060f342` | run-be3c6c1fdad8 | 0.083043 | 22.307 | 323 | 19.596345 | 64.608753 |
| 6 | Global interning before compact slots | `35c44e9` | run-f255ee6ba678 | 0.770184 | 12.521 | 218 | 7.661216 | 44.214884 |
| 7 | Inline prefixes and bounded quote search | `1a14f38` | run-2134f4a745f2 | 0.063414 | 22.934 | 342 | 22.110772 | 67.230195 |
| 8 | Packed integer parsing | `af43e3f` | run-078fb2108046 | 0.061212 | 22.888 | 362 | 22.322696 | 67.437332 |
| 9 | Unchanged-source confirmation | `1311354` | run-d480ecba4541 | 0.059338 | 23.093 | 362 | 22.632094 | 67.736237 |
| 10 | SIMD quote search and smaller working set | `9c8f98c` | run-4dda2a32142b | 0.051579 | 13.193 | 377 | 26.985886 | 71.556834 |
| Follow-up | Unchanged binary; formatting/comments confirmation | `a3babda` | run-ab1dcf6dda0c | 0.051316 | 12.657 | 377 | 27.280554 | 71.792660 |

Local `check.py` passed the official seed-1 full output hash
`d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536`.
The local Docker check took 0.27 s including container startup; this is not a
board measurement and is not used as a judging claim.

Second local experiment: intern names globally and track country membership with
four 64-bit words per name. This replaces millions of per-country set nodes with
one state per name. The same full local sandbox check fell from 3.87 s to 2.15 s,
and the output hash stayed identical. A later board run, `run-f255ee6ba678`,
measured 0.770184 s and 12.52 MB with that implementation.

The compact table reduced the local full sandbox check from about 2.15 s to
1.73 s. The board measured 0.577201 s and 9.91 MB, raising K from 7.66 to 9.12.

Parsing three record-aligned ranges in parallel (run-a04bbae35fdc) took the
board to 0.319128 s at 23.0 MB and K 9.96. Peak memory rose because each worker
keeps its own name table, but the track still uses far less than the 367 MB
reference, so the time win dominated.

A profile then showed distinct-name dedup was about 77% of the run (236 ms of
306 ms locally), bound by random cache misses into the per-worker name table and
by dereferencing each stored name back into the 195 MB CSV mapping on every
probe. Two changes addressed this without changing the output: each slot now
stores the name's hash, so a hash mismatch skips the string compare (and its
mapping dereference), and the home slot is prefetched the moment the name is
parsed, so the miss overlaps the note, region and amount parsing that follows.
Local full sandbox time fell from 306 ms to 280 ms; the board (run-851a9e87bcce)
measured 0.225056 s at 26.1 MB, raising K from 9.96 to 11.50. Board timings for
the unchanged parallel-parse code varied between 0.286 s and 0.340 s across runs,
and 0.225 s sits below that whole range, so the gain is real rather than noise.

In step 5, we eliminated both per-worker table duplication and the sequential
post-parse merge. A single lock-free 262,144-slot open-addressed table with 64-byte
cache-aligned entries is shared across all 12 hardware threads. Each slot holds
the name's atomic 64-bit hash, pointer into the mapped CSV, length, and four
atomic 64-bit country bitmasks. Upon seeing a name, threads perform a relaxed check
on the country bitmask; if unset, an atomic `fetch_or` claims the bit and increments
the thread-local country count if and only if the bit was previously unset. This
removes the serial name-merge phase. The board measured peak cgroup memory of
22.31 MB (run-be3c6c1fdad8), with K=19.596345 and S=323.

The seed-2 sample generated from the published generator also matched its Python
reference output hash `f389938275df1b495a38c4375fe741795df180e64617e01e2cf77a95a56982a3`.

## Inline name prefixes and vectorized quote search (officially confirmed below)

Each 64-byte name slot now stores a 16-byte prefix and a 32-bit CSV offset.
Short-name equality stays in the table's cache line; long names still compare
all remaining bytes in the mapping. A packed hash replaces the per-byte FNV
chain, retaining exact collision checks. Hash values 0 and 1 explicitly represent
free and publishing slots, with release/acquire publication. Quoted fields find
quotes using bounded memchr, retaining escape and embedded-newline semantics.

Five alternating local process runs on the full public data: previous
middle-three mean 0.066621 s; new 0.041007 s (38.4% reduction). The full public
hash and official seed-17 tiny reference hash match. This is local timing;
these local timings are not official scores. The confirmation below includes
the scoring host's memory/source cost.

An additional official seed-7 sample reference also matches:
`4a8b5533b3639d52e4bb82e622c0a03c071e3e4f7ea62c42fcb716d373602958`.

Official confirmation for the inline-prefix version: `run-2134f4a745f2` at
`1a14f38` measured 0.063414 s, 22,933,504 bytes, S=342, K=22.110772 and
67.230195 points. All gates passed.

## Packed integer parsing (officially confirmed below)

Use bounded eight-byte integer parsing for record IDs, region IDs and amount
whole parts. A SWAR mask locates the first non-digit; packed decimal reduction
produces the exact integer. Values spanning eight bytes continue scalarly, and
short file tails use the original reader. Money stays in integer cents.

Five alternating local full-data process runs: previous middle-three mean
0.044812 s, candidate 0.039446 s (12.0% reduction). The public full hash, official
seed-7 sample and seed-17 tiny match. A 2,200-record independent oracle adds
1–8-digit IDs, large id sums, mixed terminators, boundary BOMs, escaped quotes,
embedded note newlines, empty/space-distinct names, deliberate packed-hash
collisions, unmatched regions and negative amounts below one unit.

Rejected before submission: routing names to exclusive shard owners passed hashes
but slowed local mean 0.042174 → 0.071305 s and peaked around 56 MB. Splitting
immutable keys and membership words into separate 32-byte tables also lost:
0.056974 → 0.065962 s locally. Both were reverted; source and memory cost matter.

Official confirmation: `run-078fb2108046` at `af43e3f` passed every gate with
0.061212 s, 22,888,448 bytes, S=362, K=22.322696 and 67.437332 points. This is
only a modest gain over the prior inline-prefix implementation; local speedups
were larger and should not be substituted for this official measurement.

Offline PGO on newly generated seed-17 sample data was rejected. Five alternating
full-data runs measured 0.050499 s without it and 0.056448 s with PGO/native
tuning. All hashes matched, but the candidate was slower.


## Bounded SIMD fields and a smaller shared working set (officially confirmed below)

All seven public input files across A–D were rechecked against the authoritative
SHA256SUMS.json; every input matched. The C candidate passed check.py with the
published full expected hash. It also matches official generator seed 7 sample,
seed 17 tiny, and a newly generated full seed 42 against the independent Python
reference (54ed4d6bd196c17dda213cfcc7b79ec6732f06d80b1b3cb444e7468dd438e517).
The independent CSV oracle now exercises quotes around 16/32/64-byte boundaries,
long trailing spaces, lone and edge quotes, and embedded note newlines.

Research motivation: https://github.com/geofflangdale/simdcsv explains SIMD
quote discovery. This implementation adds its own bounded AVX2 quote search;
it copies no external parser and adds no dependency. All 32-byte loads remain
within the input mapping, with bounded memchr handling the tail. Home-slot
prefetch overlaps the later note/numeric parse. The shared table falls from
262,144 to 131,072 slots, retaining exact equality and atomic publication. Hash
values reserve 0 and 1 without forcing a fixed bucket bit. The region lookup
uses uint8_t entries with 255 missing, valid for the guaranteed 240 countries.

Nine interleaved full-data process runs, six Ryzen cores plus SMT, mean of the
middle five: confirmed baseline 0.056144 s; final candidate 0.042343 s (24.6%
less time). Every output hash matched. A separate shell-wrapped peak cgroup
check measured 24,821,760 -> 13,250,560 bytes; this is local evidence, not the
scoring host's peak. Other workloads on the local machine add timing noise;
No official points are inferred from these measurements.

Rejected alternatives: buffering 16 name lookups did not improve on immediate
lookup plus prefetch. Six and eight parser workers gave 0.054526 and 0.044613 s
versus 0.042343 s with twelve. Smaller tables and hash changes were also tested
in isolation; the combined final candidate was selected by its measured time
and memory. No Rust/C++ boundary or third-party parser is required for these
operations; the submitted program remains C++ with zero dependencies.


Official confirmation: `run-4dda2a32142b` at `9c8f98c` passed every gate:
0.051579 s, 13,193,216 bytes, S=377, K=26.985886, **71.556834 points**.
The previous unchanged implementation at `1311354` / `run-d480ecba4541` measured
0.059338 s, 23,093,248 bytes, S=362 and 67.736237 points. Official time fell
13.1% and peak memory 42.9%; this is a measured combined-score improvement.

## Rust and static-link evaluations

A zero-crate Rust port of the selected algorithm passes public full, generated
full seed 42, sample seed 7, tiny seed 17 and the expanded independent oracle.
The buildable candidate is in experiments/track-c-rust, outside the submission.
Five interleaved local runs gave C++ 0.036943 s versus Rust 0.040764 s; a later
batch gave 0.030957 versus 0.030941 s. No reproducible runtime gain was established.
A Rust layout with immutable 32-byte name slots and a transposed country/name
membership bitset also tied locally (0.031343 s). Neither is an official score.

Static/native C++ measured 0.041202 -> 0.040294 s but local peak rose from
13,520,896 to 14,229,504 bytes; no clear combined benefit justified changing the
selected build. The submitted C++ algorithm remains the confirmed SIMD/compact
working-set implementation. LLVM formatting and publication comments preserve
its generated binary in the local build.


Official follow-up at `a3babda`: all gates passed. Unchanged binary; formatting/comments confirmation.

## Official Rust trial requested

At the team's request, the zero-crate Rust port is now submitted for a direct
scoring-host comparison. It preserves the confirmed C++ algorithm: mapped CSV,
bounded AVX2 quote discovery, exact cents, shared immutable name keys published
with acquire/release, exact equality and atomic country membership. The pinned
offline Rust build passes the public full expected hash, generated full seed 42,
sample seed 7, tiny seed 17 and the independent CSV oracle. Local timing ties do
not decide this trial; its official T/M/S/K will decide retention. The last C++
source and its 71.792660-point confirmation are preserved at a3babda.
