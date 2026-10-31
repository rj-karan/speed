# Track A: Ingest

The task, the input files and the exact output format are in the [spec](https://github.com/hackathon-raptors/speed-demon-2026/blob/main/specs/a-ingest.md).

## Approach

The C++ program memory maps the log and parses only the fixed-width hour, endpoint,
status class, and unsigned latency. It interns each endpoint as a view into the input
mapping, then collects latencies in one vector per (endpoint, hour). This handles
out-of-order shards and duplicate lines without assuming a sorting window. After
reading, it sorts endpoints by byte order and each populated group's latency vector.
The nearest-rank percentiles and error rate are computed with integers, then emitted
in hour and endpoint order.

## Dependencies

None.

## With another 24 hours

Profile the board run, then compare full sorting against partial selection for the
three percentile ranks. The latter might save time on the busiest groups.
