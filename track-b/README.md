# Track B: Index (BeyondBug)

## Approach

Our Track B solution is implemented in pure standard-library Rust with zero runtime dependencies ($D = 1$).

### Key Architectural Decisions

1. **Query-First Term Interning**:
   Only terms that appear in queries are interned and tracked. Any document tokens not in the query vocabulary are quickly bypassed with zero allocation.
2. **Open-Addressing Term Hash Table**:
   Terms are indexed using an open-addressing table with FNV-1a hashing for fast $O(1)$ lookups without memory allocator overhead.
3. **12-Thread Parallel Document Indexing**:
   Documents are partitioned across all 12 worker threads. Thread-local posting lists are combined into pre-allocated contiguous vectors without requiring sorting.
4. **12-Thread Parallel Query Evaluation**:
   Queries are dynamically distributed using relaxed atomic fetch-add work stealing.
5. **Short-Circuit Boolean & Phrase Verification**:
   Boolean operators evaluate left-to-right with sorted two-pointer intersection, union, and difference. Phrase queries search candidate documents starting from the rarest term and verify token sequences.
6. **Zero Dependencies**:
   $D = 1$ to maximize the dependency scoring term $(1/D)^{0.15} = 1.0$.
