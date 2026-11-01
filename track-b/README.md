# Track B: compact hybrid index

Read queries first, intern their terms and tokenize ASCII alphanumeric runs with
bounded SIMD loads. A packed hash checks original bytes on collisions. No Unicode
normalization or stopword removal is performed.

The first corpus pass counts unique document occurrences per queried term. Allocate
one exact contiguous posting array for rare terms and bitmaps for frequent terms,
choosing whichever representation uses fewer bytes. Sparse IDs use two bytes
relative to each worker's base. Segment ends reconstruct exact global IDs and
limit phrase membership searches to the correct worker segment. At least four
aligned ranges cover the specified maximum of 250,000 documents within 16 bits;
a runtime guard checks the span. The second pass fills these
arrays directly. Document ranges align to bitmap words, so workers write disjoint
locations without atomics. This eliminates duplicated posting vectors and their
serial concatenation. All allocations derive from current input, not cached data.

Boolean operators apply strictly left to right to per-worker bitmap scratch:
dense terms use word operations; sparse terms touch their listed documents.
Phrase queries visit candidates from the rarest term and verify adjacent tokens
around that anchor in raw text. Case, punctuation, repeated terms and UTF-8 byte
separators follow the frozen spec exactly. Output order follows query order.

## Dependencies and verification

No non-standard-library runtime dependencies (D = 1). Full public hash, official
seed-2 sample, seed-17 tiny reference, and the independent 2,500-query oracle pass.
Local comparisons, rejected candidates and official confirmations are in PERF.md.

## With another 24 hours

Profile alternatives to the second tokenize/hash pass while controlling temporary
storage. Compare time, cgroup peak and formatted source cost; one-pass and
anchor-directed prototypes have already failed that combined comparison.

The tokenizer classifies 32-byte blocks and emits start/end transitions, carrying
partial tokens across blocks and respecting the document tail. Sixteen-token
batches prefetch dictionary slots. One-to-four-letter words use an exact 20-bit
key; digits and longer words retain full byte equality checks. Phrase candidates
first pass posting-membership checks, then matching documents contribute directly
to count, sum and the first ten IDs without a document-sized answer bitmap.
