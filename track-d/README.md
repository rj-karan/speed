# Track D: locality-aware bidirectional Dijkstra

## Approach

Twelve workers count forward/reverse degrees in shared atomic arrays, then reuse
those arrays as CSR write cursors. Atomic increments assign disjoint output
positions; joining the builders completes each phase before the next begins.
Shared counters avoid replicated per-worker degree/cursor arrays.

Renumber every vertex in breadth-first order and rebuild both CSR arrays in that
order. Remap all query endpoints with the same bijection. Every directed edge,
weight, duplicate, parallel edge and isolated node is retained; the transformation
changes only memory layout. All preprocessing is inside the timed run.

Four workers answer queries with exact bidirectional Dijkstra and monotone radix
heaps. A uint64 state word stores an exact 53-bit distance and an 11-bit query
stamp. For every published scale, n*UINT32_MAX is below 2^53, including one extra
relaxation edge. The constructor checks that bound. Stamps are explicitly cleared
at wraparound. This reduces memory traffic without truncating path sums.

## Research and verification

[PHAST's authors](https://www.microsoft.com/en-us/research/wp-content/uploads/2010/09/phastTR.pdf)
discuss how vertex order affects shortest-path locality. We implemented and timed
both DFS and BFS numbering ourselves; BFS won on this generator, including its
preprocessing cost. No routing or shortest-path library is used.

Full public output, official seed-7 sample and seed-17 tiny reference outputs,
and an independent heapq oracle match. A 10,000-query fixture forces stamp
wraparound and checks sums above uint32. Unreachable, zero-weight, duplicate and
directed cases remain exact. No non-standard-library runtime dependencies (D=1).

## With another 24 hours

Investigate compressed edge storage with exact handling of exceptional weights,
and measure whether it repays its encoding/preprocessing and source-size costs.
