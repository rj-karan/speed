# Track B: single-pass compact hybrid index

Read queries first, intern their terms and tokenize ASCII alphanumeric runs with
bounded SIMD loads. Exact short-letter encoding and collision-checked hashing
preserve original token semantics. No Unicode normalization or stopword removal.

A deterministic sample of current documents chooses sparse lists versus dense
bitmaps. It affects storage only; every document is indexed exactly. Twelve
workers tokenize/hash their disjoint ranges once, filling dense bitmaps directly
and caching sparse term IDs in variable-width packed streams. Width derives from
the current query dictionary. Per-document cumulative ends reconstruct the exact
document for every hit without storing its ID beside every cached term.

Mapped caches reserve a safe token upper bound (a length-L text has at most
ceil(L/2) tokens), touch only written words and are explicitly released. Final
sparse postings use two-byte IDs relative to worker bases. They are allocated
worker-major: one worker scatters into its own contiguous final segment, then
unmaps its cache. This avoids faulting the entire final output while all caches
are live. Per-worker term ends delimit lists; the previous term's end or the
worker base gives the start, including dense/absent terms with empty segments.
At least four aligned ranges cover the maximum 250,000 documents within 16 bits;
a runtime guard checks the span. Bitmap ranges align to words, so writes need no
atomics. All storage and decisions derive from the current files.

Boolean operators apply strictly left to right to scratch bitmaps. A dense first
operand is copied directly. Phrase candidates use the rarest term, posting
membership filters and exact adjacent-token checks in raw text. Matches contribute
count, uint64 sum and the first ten sorted IDs; output follows query order.

## Dependencies and verification

No external runtime dependencies (D = 1). The pinned GCC/Debian images build and
run offline. Full public output, three additional full-scale references and the
independent 2,500-query byte-token oracle cover punctuation, uppercase, UTF-8
separators, repeated terms and operator order. PERF.md distinguishes local
measurements from official scoring-host results.

## With another 24 hours

Profile parsing, indexing, scatter and queries on the scoring CPU. Compare cache
widths and representation thresholds with time, cgroup peak and canonical source
cost. Retain changes only when official combined K improves.
