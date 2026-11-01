# BeyondBug performance evidence

Track histories: [A](track-a/PERF.md), [B](track-b/PERF.md),
[C](track-c/PERF.md), [D](track-d/PERF.md). Each table records official run IDs,
commit, process time, decimal MB, canonical source S, coefficient K and points.
Regressions and rejected prototypes are labelled separately. Unchanged code can
vary between runs; repeated confirmations are not new algorithm improvements.

## Best confirmed machine results

These are historical bests before judging. They
come from different commits; each track log identifies the measured source.

| Track | Commit | Run | T (s) | M (bytes) | S | K | Points |
|---|---|---|---:|---:|---:|---:|---:|
| A | 9c8f98c | run-24951b257cd0 | 0.046647 | 23,543,808 | 323 | 31.734877 | 75.076841 |
| B | a9b5f40 | run-2f033ee2f246 | 0.481606 | 92,770,304 | 639 | 15.210464 | 59.107124 |
| C | a3babda | run-ab1dcf6dda0c | 0.051316 | 12,656,640 | 377 | 27.280554 | 71.792660 |
| D | 9c8f98c | run-3789c40c96ef | 0.620974 | 228,958,208 | 345 | 10.393619 | 50.838341 |

The four-byte A trial at 77939f3 passed all gates but regressed to 71.783869
points (0.062165 s, 21,741,568 bytes, S=379). It is rejected. A returns to its
9c8f98c executable statements/build (LLVM formatting and comments only);
C uses its a3babda main.cpp source. B retains the officially improved mapped cache with worker-major
final sparse segments, following phase-memory measurements recorded in its log. D keeps the measured BFS-order/packed-state design with
diagnostics. Local improvements are never substituted for scoring-host results.

## Validation

All seven seed-1 input SHA-256 values match the published SHA256SUMS.json. All
four current images pass check.py with the published full expected output hash.
The seven independent oracle tests pass. C additionally matches generated full
seed 42, sample seed 7 and tiny seed 17 against the official Python reference.
The Rust C evaluation passes these same hashes and the CSV oracle; it is retained
outside the submitted track folders because it did not establish a local gain.

`tools/validate_seeds.py` completed three additional full-scale seeds (19, 37,
59), twenty sandbox executions per track/dataset: **12 datasets and 240 matching
runs for the selected source images**. B/C and restored A were rechecked after
changes. Reports pin each image by immutable ID, verify the exact output file
set, delete prior results and compare every output SHA-256 with the official
Python reference. [Validation record](validation/full-seeds.json) includes input,
generator/reference and submitted-source hashes. Repetition is stability evidence,
not a formal race-freedom proof. The independent oracles also pass.

The cached-hit B/control-metadata C trial at 827cf69 passed correctness but
lost combined K: B 57.856675 and C 68.269914 points. B returns to the confirmed
compact two-pass source. C's isolated boundary-word follow-up at fe49a2b also regressed to
70.820049 points and is reverted to a3babda. These selected images were rechecked
twenty times on each additional full seed; the validation record is refreshed.
The direct Rust port's 70.889639-point trial is also recorded and rejected against
the prior C++ result. The public board's historical best does not replace testing
the source that will actually be submitted at freeze.

## Reading the results

Local timings use the official process timer, prewarmed full inputs, interleaved
candidates and six local physical Ryzen cores plus SMT. Correctness checks use
twelve permitted logical CPUs and the event's privilege, filesystem, network,
8 GB/no-swap, process and temporary-filesystem limits. These checks and local
experiments are separate from official scoring-host measurements.

B remains the weakest counted track. Improving D alone does not raise the overall
machine score while it remains below B. The final result also includes judging;
comments and clean evidence support review but do not guarantee judging points.


## Submitted-state confirmation

All four tracks at `a9b5f40` passed all official gates. B gains 1.202688 machine
points over its previous best. Its 23.3% time reduction outweighs higher memory
and source cost. The board's best-per-track geometric mean moves from 67.832 to
68.298; the single exact-commit run's best-three mean is 68.178. This does not
establish a top-three result. A/C regressions were rejected and their confirmed
algorithms restored. D remains the weakest excluded track.

Each track log ends with its source commit/run/T/M/S/K. The
[official snapshot](validation/official-latest.json) records the measured results
and historical bests. [Local B comparison](validation/b-local-comparison.json)
pins both images, timer, input hashes and raw interleaved samples. Reproduce local
comparisons with [tools/benchmark.py](tools/benchmark.py), as described in README.
Comments/LLVM formatting on restored A produce the identical runtime image and
canonical S=323. Documentation/tool-only follow-ups change no submitted source.
