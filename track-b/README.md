# Track B: Index

The [frozen specification](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/b-index.md) defines the task and byte-level tokenizer.

## Approach

Read the queries first and intern only their terms. Up to twelve workers scan
disjoint document ranges in the mapped corpus, tokenize ASCII alphanumeric runs,
and build document-id postings. Hash probes always check the original bytes;
collisions cannot change an answer. Concatenating worker ranges yields sorted
postings with one entry per term per document.

Boolean operators apply strictly left to right. Similar-sized posting lists use
linear intersection/difference; when one list is over 32 times larger, binary
search skips its irrelevant ranges. Union merges sorted lists.

Phrase queries use the rarest term's postings to find candidate documents.
An AVX2 scan locates that term's first byte, ignoring ASCII case; exact comparisons
and token boundaries validate each hit. Check the required neighbouring tokens
backward and forward from the anchor. Repeated terms must occupy separate,
consecutive positions. All non-ASCII bytes remain separators, including bytes of
UTF-8 letters. Every vector load stays inside the document, and the final bytes
use a scalar scan. This avoids hashing every candidate token against every phrase
term, without storing a positional index or allocating per candidate.

Independent queries run in parallel; results are written in query order. The
worker cap is twelve because hardware_concurrency can report the host's CPUs
rather than the container's allowed CPUs.

## Dependencies and validation

No non-standard-library runtime dependencies (D = 1). Compiler intrinsics target
the published AVX2 CPU through the existing x86-64-v3 build.

`PERF.md` separates official board measurements from local experiments. The full
seed-1 hash, official seed-2 sample and seed-17 tiny reference outputs, and the
2,500-query differential test in `tests/test_index.py` check correctness. Run the
test from the repository root after building the Track B image.

## With another 24 hours

Profile index construction separately from query evaluation on the scoring CPU.
Compare compact contiguous posting storage with the current per-worker vectors,
and measure both peak cgroup memory and source-size cost before keeping it.
