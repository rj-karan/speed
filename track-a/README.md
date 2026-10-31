# Track A: Ingest (BeyondBug)

## Approach

Our Track A solution is implemented in pure standard-library Rust with zero runtime dependencies ($D = 1$).

### Key Architectural Decisions

1. **Memory-Mapped Input (`mmap`)**:
   Input `/data/requests.log` is mapped directly to address space for zero-copy parsing.
2. **12-Thread Parallel Chunk Parsing**:
   The log file is partitioned across all available CPU threads with newline-aligned chunk boundaries.
3. **Open-Addressing L1/L2 Endpoint Interning**:
   Endpoints are interned into thread-local hash tables without heap allocations per line.
4. **Counting / Radix Scatter**:
   Latencies and 5xx errors are accumulated in thread-local buffers and scattered into a contiguous global array in a single pass.
5. **Parallel Bucket Sorting**:
   Non-empty (slot, endpoint) slices are sorted in parallel across all 12 threads using `sort_unstable()`.
6. **Zero Dependencies**:
   $D = 1$ to maximize the dependency scoring term $(1/D)^{0.15} = 1.0$.
