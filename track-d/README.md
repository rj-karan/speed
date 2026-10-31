# Track D: Traverse (BeyondBug)

## Approach

Our Track D solution is implemented in pure standard-library Rust with zero runtime dependencies ($D = 1$).

### Key Architectural Decisions

1. **Dual Compressed Sparse Row (CSR)**:
   Builds forward and backward CSR graph representations directly from memory-mapped edge data with a single counting pass.
2. **12-Thread Parallel Bidirectional Dijkstra**:
   Queries are dynamically distributed across 12 worker threads using atomic fetch-add work stealing.
3. **Generation-Stamped Search States**:
   Each worker thread maintains pre-allocated forward and backward distance vectors with a 32-bit generation counter. This eliminates per-query memory allocations and array clearing overhead.
4. **Contiguous Binary Min-Heap**:
   Custom inline binary min-heap over a contiguous vector eliminates pointer chasing and heap fragmentation.
5. **Early Termination**:
   Bidirectional search settles the smaller boundary first and breaks immediately once the sum of top-of-heap costs meets or exceeds the best path discovered.
6. **Zero Dependencies**:
   $D = 1$ to maximize the dependency scoring term $(1/D)^{0.15} = 1.0$.
