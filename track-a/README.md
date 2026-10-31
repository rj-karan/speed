# Track A: hour-local aggregation

## Approach

The dependency-free C++ program maps the input and discovers every contiguous
run of one hour using vectorized newline scanning. Runs are indexed by hour;
no bound on input disorder is assumed. Twelve persistent workers then claim
hours independently. Each hour has its own endpoint dictionary and compact
six-byte latency/endpoint records. Exact byte comparisons resolve hash collisions.
A counting scatter groups the latencies in a small hour-local array. Nearest-rank
selection extracts p50/p95/p99; small groups use sorting. Endpoint keys are sorted
bytewise, and hour buffers are written in chronological order.

There are no shared per-request atomics, global latency scatter cursors, or
cross-worker name-table merges. Temporary records are discarded after their
hour is formatted. AVX2 finds endpoint delimiters; bounded packed-decimal parsing
handles up to seven latency digits at once, with a scalar path for longer values
and file tails. Full uint32 latency values and duplicate requests remain exact.

## Build and validation

The allowlisted GCC image builds offline with `-march=native`; the event explicitly
builds native-targeted tracks on its scoring machine, so the resulting binary
matches that CPU. No non-standard-library runtime dependencies (D = 1).

Validation includes the full public hash, official seed-17 sample reference,
and `tests/test_ingest.py`: shuffled hours, all methods, byte-order-sensitive
endpoints, endpoint lengths around vector boundaries, every decimal digit length,
uint32 extremes, duplicates and empty input. PERF.md distinguishes official
board results from local experiments.

## With another 24 hours

Profile range discovery, parsing and output separately on the scoring CPU.
Compare direct mapped output and profile-guided compilation using generator
output. Keep changes only when the combined time/memory/source coefficient improves.
