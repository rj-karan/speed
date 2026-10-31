# Track C: Join

The task, the input files and the exact output format are in the [spec](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/c-join.md).

## Approach

The C++ program memory maps both input files. It loads the plain region table into
an indexed array and parses records in one pass. The parser skips BOMs only at
record boundaries; it treats quoted newlines as data and counts escaped quotes
as one byte. Amounts remain exact integer cents. Aggregates and distinct-name
counts are stored per country. Names are interned once, with a 241-bit country
membership mask per distinct name. Name keys refer to raw input slices: generated
nonempty names are always quoted, so this is a one-to-one representation of the
unescaped value; quoted and unquoted empty names both have an empty slice.
An open-addressed table keeps those states contiguous. It resolves hash collisions
with exact byte comparisons with 262,144 fixed slots for the specified 100,000-name pool.
Up to twelve workers parse record-aligned ranges. A partition boundary follows an LF
only when the next byte begins an id or boundary BOM; generated note lines start
with lowercase letters. Workers share the name table; an atomic country-bit claim counts a
name once per country. Worker numeric aggregates are summed after parsing.
Output scans country codes in byte order and writes the unmatched group last.

## Dependencies

None.

## With another 24 hours

Profile the parser and name lookup separately on the scoring box, then compare
worker counts and table capacities under the cgroup memory metric.
