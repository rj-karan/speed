# Track D: Traverse

The task, the input files and the exact output format are in the [spec](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/d-traverse.md).

## Approach

The C++ program memory maps the graph and parses it twice. The first pass counts
outgoing edges; the second fills a compact CSR adjacency array without retaining
a second copy of the edges. Each query runs an early-exit Dijkstra search with
64-bit distances and generation-stamped state, so clearing all node distances per
query is unnecessary. Zero-weight, parallel, duplicate, and directed edges are
kept exactly. Nodes with no outgoing edges and targets with no incoming edges are
rejected early, while identical source and target returns zero.

## Dependencies

None.

## With another 24 hours

Profile heap operations and test a radix heap or a bidirectional search on the
published generator's local query distribution, including the unreachable cases.
