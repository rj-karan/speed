# Track B: Index

The task, the input files and the exact output format are in the [spec](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/b-index.md).

## Approach

The C++ program reads the queries first and interns only terms that are actually
queried. It memory maps the corpus, tokenizes ASCII bytes exactly as specified,
and builds sorted document-id posting lists only for those terms. Six workers scan
disjoint document ranges, then merge their posting lists in document order. Repeated terms
within a document add one posting. Boolean operators use sorted set intersection,
union, and difference strictly from left to right. Phrase queries begin with the
rarest term's posting list, check the other terms' postings, and then re-tokenize
candidate documents to verify consecutive positions. Output counts, sums and the
smallest ten ids are computed from the resulting sorted document list. Six query
workers evaluate independent queries and write their results in original order.

## Dependencies

None.

## With another 24 hours

Profile common-term Boolean chains and test a compact bitset for frequent query
terms. Phrase candidates are already sparse, so the Boolean path is the likely
bottleneck at full scale.
