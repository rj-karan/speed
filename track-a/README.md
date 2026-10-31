# Track A: Ingest (BeyondBug)

## Approach

A dependency-free Rust program maps the log and splits it at newline boundaries
across up to twelve persistent workers. Fixed-offset timestamp parsing maps the
specified 72-hour span to hour slots, including the month boundary. SWAR scanning
finds endpoint delimiters; a shared atomic table interns endpoints, with exact
byte comparisons on hash collisions and small thread-local lookup caches.

The first scan counts requests and errors. After prefix sums allocate exactly
one latency array, a second scan scatters latencies into contiguous group slices.
Workers sort each slice and extract the three nearest-rank percentiles. Output
is formatted in parallel by hour and concatenated in hour/endpoint byte order.
Barriers separate counting, allocation, scattering and formatting.

The final code restores the implementation measured as run-7912c18c175d. A later
thread-local cursor experiment increased formatted source from 484 to 689 lines
without improving board time; PERF.md records why it was reverted. The earlier
version's smaller memory footprint and source size produced a higher K.

## Dependencies and validation

No runtime libraries beyond Rust's standard library and Linux mmap (D = 1).
The Docker build uses the allowlisted compiler offline. Full public output must
match `495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3`.

## With another 24 hours

Profile atomic count/scatter contention and compare percentile selection with
sorting. Keep changes only when time, peak memory and formatted source size
improve the combined coefficient, and validate on additional generator seeds.
