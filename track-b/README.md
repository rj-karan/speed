# Track B: compact hybrid index

Read queries first, intern their terms and tokenize ASCII alphanumeric runs with
bounded SIMD loads. A packed hash checks original bytes on collisions. No Unicode
normalization or stopword removal is performed.

A small sample chooses bitmap representation for frequent terms; sampling never
filters answers. One complete corpus pass fills those bitmaps and caches every
unique sparse (term, document) occurrence. Cached IDs use the dictionary's actual
bit width, with per-document cumulative ends supplying document IDs. Exact counts
then allocate two-byte worker-relative postings, filled from the cached streams.
Each consumed stream is released while scattering. This removes the second full
text tokenize/hash pass. Aligned worker ranges keep bitmap writes disjoint, and
at least four ranges bound local document IDs to sixteen bits at full scale.

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
