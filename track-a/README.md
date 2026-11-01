# Track A: single-pass compact aggregation

## Approach

Twelve workers parse disjoint line-aligned byte ranges once. Each worker interns
exact endpoint bytes and stores private records by hour. A record uses 11 bits
for an endpoint ID, one for 5xx status and 20 for latency. Values at or above the
20-bit sentinel are retained in a full-width overflow stream in encounter order;
this is exact compression, including UINT32_MAX, rather than truncation.
Private endpoint counts use a low byte plus multiples of 256 in a spill table.

Two barriers publish the private dictionaries and global endpoint mappings.
Workers then claim whole hours. Counts determine a direct scatter into contiguous
per-endpoint latency-code ranges, after which the private hour buffers are freed.
Nearest-rank selection operates on those ranges. If a selected rank reaches the
sentinel, the exact rank within that endpoint's full-width tail is selected.
Endpoints are sorted bytewise and output hours are written chronologically.
No bound on input disorder is assumed. Each hour has a single owner; there are
no shared request counters or scatter cursors.

AVX2 finds endpoint delimiters. Bounded SWAR decimal reduction handles short
latencies; a scalar path handles longer values and file tails. Hash collisions
always require exact endpoint equality. Mapped record buffers grow on demand;
output mappings are released after writing, avoiding retained output allocations.

## Build and validation

The allowlisted GCC image builds offline with `-march=native`; the event builds
native-targeted tracks on its scoring CPU. No external runtime dependencies
(D = 1). `check.py` verifies the published full output hash. Independent tests
cover shuffled hours, every method, endpoint/vector boundaries, duplicate lines,
empty input, uint32 extremes, sentinel-adjacent values, quantile ranks inside
overflow tails and 800,000 repeated requests forcing buffer growth/count wraps.
Three additional full seeds are checked against the official Python reference.
PERF.md separates local experiments from official scoring-host results.

## With another 24 hours

Profile parsing, scatter, selection and output on the scoring CPU. Compare the
memory/time/source coefficient of overflow layouts and dictionary lifetimes.
Keep only candidates that improve the measured combined coefficient.
