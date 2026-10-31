# Track A: single-pass parsing with mapped record buffers

## Approach

Map the input and split it at newline boundaries across twelve persistent
workers. Each worker parses its byte range once, interns endpoints in a private
collision-checked dictionary, and appends six-byte latency/key records to
hour-specific anonymous mappings. The key also carries the error bit. Mappings
grow when needed; no bound on input disorder or per-hour request count is assumed.

After parsing, merge only the small endpoint dictionaries and sort their keys
bytewise. Workers then claim whole hours, sum local counts, scatter that hour's
records into contiguous group slices and release its record mappings. Exact
nearest-rank selection extracts the three percentiles. Output uses bounded mapped
buffers that are released after their ordered writes. This removes the previous
whole-input hour-discovery scan without relying on allocator reclamation.

AVX2 finds endpoint delimiters; packed decimal parsing handles short latencies,
with scalar handling for longer values and the file tail. All equality checks
compare complete endpoint bytes. Duplicate lines and uint32 extremes remain exact.

## Dependencies and validation

No non-standard-library runtime dependencies (D = 1). Native compilation uses the
allowlisted GCC toolchain; the event builds it on the scoring CPU. Full public
output, an additional official seed and the independent oracle match. The oracle
also forces record mappings to grow with 800,000 duplicate requests, and tests
month/hour boundaries, ordering, all methods, empty input and decimal lengths.

## With another 24 hours

Profile parsing, scatter, percentile selection and output separately on the
scoring CPU. Keep changes only when total time, kernel peak and formatted source
size improve the coefficient. An endpoint string-pool experiment was rejected
locally because it was slower than the mapped-buffer design.
