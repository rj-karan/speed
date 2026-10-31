# Track C: Join

The task, the input files and the exact output format are in the [spec](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/c-join.md).

## Approach

The Rust program memory maps both input files. It loads the plain region table into
an indexed array and parses records in one pass. The parser skips BOMs only at
record boundaries; it treats quoted newlines as data and counts escaped quotes
as one byte. Amounts remain exact integer cents. Aggregates and distinct-name
counts are stored per country. Names are interned once, with a 241-bit country
membership mask per distinct name. Each table slot caches a 16-byte name prefix and a CSV offset. Name keys refer to raw input slices: generated
nonempty names are always quoted, so this is a one-to-one representation of the
unescaped value; quoted and unquoted empty names both have an empty slice.
A shared open-addressed table keeps those states contiguous in 131,072 fixed slots
for the specified 100,000-name pool. Exact byte comparisons resolve collisions.
Up to twelve workers parse record-aligned ranges. A partition boundary follows an LF
only when the next byte begins an id or boundary BOM; generated note lines start
with lowercase letters. Workers share the name table; an atomic country-bit claim counts a
name once per country. Worker numeric aggregates are summed after parsing.
The region array uses one byte per entry: the guaranteed maximum is 240 country
indexes, and 255 marks a missing region. AVX2 quote searches load only when all
32 bytes remain mapped, falling back to bounded memchr near EOF. Hash lookup is
prefetched while note and numeric fields are parsed.
Output scans country codes in byte order and writes the unmatched group last.

## Dependencies

None.

## With another 24 hours

Profile the parser and name lookup separately on the scoring box, then compare
worker counts and table capacities under the cgroup memory metric.

Record integers use a bounded eight-byte decimal fast path, with scalar handling
for longer values and short file tails. The independent CSV oracle in
`tests/test_join.py` checks quoting, collision equality, integer lengths, byte
counts, distinct names and signed money against direct Python aggregation.


## Rust scoring-host comparison

This trial ports the confirmed C++ algorithm to Rust without crates. Publication
uses AtomicU64 acquire/release ordering and UnsafeCell only for key fields written
by the successful publishing claimant. Exact slice comparisons resolve collisions.
Linux mmap is the only foreign function; data processing stays in Rust. The C++
comparison is preserved at commit a3babda. See PERF.md for the measured decision.
