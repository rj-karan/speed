# Track D: sparse bidirectional Dijkstra

Parse the graph into forward and reverse CSR arrays using four workers. Each
query uses exact 64-bit bidirectional Dijkstra, stopping when the two frontier
bounds meet the best discovered route. Preserve zero-weight, duplicate,
parallel and directed edges. Reject impossible endpoints early, and answer
identical endpoints with zero. No heuristic changes path lengths.

Each frontier uses a 65-bucket integer radix heap. Nonnegative weights guarantee
monotone extracted costs; redistributing a bucket moves its items into lower
buckets. Exact stale-entry checks remain in place. Full-node distance arrays use
generation stamps to avoid clearing between queries. Four workers answer
independent queries, and output is written in input order. Generation wrap
clears stamps explicitly. The CSR cursor loop also stops at u < n, avoiding an
out-of-bounds degree read at the sentinel.

An eight-worker sparse-state experiment was rejected: although faster, peak
memory rose to roughly 546 MB locally, which hurt the combined coefficient.

## Dependencies and validation

No non-standard-library runtime dependencies (D = 1). The full public output,
official seed-17 tiny reference, and an independent heapq oracle covering
unreachable, repeated, zero-weight and >32-bit distances match.

## With another 24 hours

Measure compact edge encoding and radix-bucket allocation on the scoring CPU.
Keep exact fallback storage for unusually large weights and compare total
preprocessing cost, search time and kernel peak memory.
