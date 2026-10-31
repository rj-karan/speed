# Track C: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 14.44 | 367 | 0 |
| 1 | Replace per-record Python objects with a mapped CSV parser, indexed join, and globally interned names with country membership bits | run-9f1d47d29d62 | 0.906522 | 12.34 | 42.51 |
| 2 | Store each distinct name in a compact open-addressed table, retaining bytewise equality checks | run-4bba7972a60f | 0.577201 | 9.91 | 48.01 |
| 3 | Parse three record-aligned ranges in parallel and merge name membership masks | run-a04bbae35fdc | 0.319128 | 23.0 | 49.91 |
| 4 | Store each name's hash in its dedup slot and prefetch the slot right after the name is parsed | run-851a9e87bcce | 0.225056 | 26.1 | 53.04 |
| 5 | Single lock-free global atomic 262k-slot open-addressed table with atomic bitwise fetch_or country tracking across 12 threads | run-be3c6c1fdad8 | 0.083043 | 22.31 | 64.61 |

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

## Inline name prefixes and vectorized quote search (board confirmation pending)

Each 64-byte name slot now stores a 16-byte prefix and a 32-bit CSV offset.
Short-name equality stays in the table's cache line; long names still compare
all remaining bytes in the mapping. A packed hash replaces the per-byte FNV
chain, retaining exact collision checks. Hash values 0 and 1 explicitly represent
free and publishing slots, with release/acquire publication. Quoted fields find
quotes using bounded memchr, retaining escape and embedded-newline semantics.

Five alternating local process runs on the full public data: previous
middle-three mean 0.066621 s; new 0.041007 s (38.4% reduction). The full public
hash and official seed-17 tiny reference hash match. This is local timing;
no official score is claimed yet, and peak memory/source cost must be weighed
in the board result.

An additional official seed-7 sample reference also matches:
`4a8b5533b3639d52e4bb82e622c0a03c071e3e4f7ea62c42fcb716d373602958`.

Official confirmation for the inline-prefix version: `run-2134f4a745f2` at
`1a14f38` measured 0.063414 s, 22,933,504 bytes, S=342, K=22.110772 and
67.230195 points. All gates passed.

## Packed integer parsing (board confirmation pending)

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
