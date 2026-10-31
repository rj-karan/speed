# BeyondBug performance evidence

Track histories: [A](track-a/PERF.md), [B](track-b/PERF.md),
[C](track-c/PERF.md), [D](track-d/PERF.md). Each table records official run IDs,
commit, process time, decimal MB, canonical source S, coefficient K and points.
Regressions and rejected prototypes are labelled separately. Unchanged code can
vary between runs; repeated confirmations are not new algorithm improvements.

## Last confirmed baseline before the current follow-up

All gates passed at `9c8f98c`. These are machine points, before judging.

| Track | Run | T (s) | M (bytes) | S | K | Points |
|---|---|---:|---:|---:|---:|---:|
| A | run-24951b257cd0 | 0.046647 | 23,543,808 | 323 | 31.734877 | 75.076841 |
| B | run-a8a26830f3fb | 0.646041 | 95,813,632 | 530 | 13.419566 | 56.386923 |
| C | run-4dda2a32142b | 0.051579 | 13,193,216 | 377 | 26.985886 | 71.556834 |
| D | run-3789c40c96ef | 0.620974 | 228,958,208 | 345 | 10.393619 | 50.838341 |

The follow-up adds smaller B postings, static linking in A, named A calendar
constants, bounded B document-header scanning, diagnostic errors, algorithm and
publication comments, and LLVM formatting. C's generated runtime binary stays
identical after formatting/comments. Performance changes require a fresh board
confirmation; these baseline figures are not attributed to them.

## Validation

All seven seed-1 input SHA-256 values match the published SHA256SUMS.json. All
four current images pass check.py with the published full expected output hash.
The seven independent oracle tests pass. C additionally matches generated full
seed 42, sample seed 7 and tiny seed 17 against the official Python reference.
The Rust C evaluation passes these same hashes and the CSV oracle; it is retained
outside the submitted track folders because it did not establish a local gain.

`tools/validate_seeds.py` is running three additional full-scale seeds (19, 37,
59), twenty sandbox executions per track/dataset. It pins each Docker image by
immutable ID, checks the exact output file set, deletes the old result before
each run and compares every output SHA-256 to the official Python reference.
Machine-readable manifests in ignored data/holdouts record input hashes,
generator/reference source hashes, image IDs and completed repetitions. A
completion statement will be added when all 240 runs pass. Repetition is
stability evidence, not a proof that data races are impossible.

## Reading the results

Local timings use the official process timer, prewarmed full inputs, interleaved
candidates and six local physical Ryzen cores plus SMT. Correctness checks use
twelve permitted logical CPUs and the event's privilege, filesystem, network,
8 GB/no-swap, process and temporary-filesystem limits. These checks and local
experiments are separate from official scoring-host measurements.

B remains the weakest counted track. Improving D alone does not raise the overall
machine score while it remains below B. The final result also includes judging;
comments and clean evidence support review but do not guarantee judging points.
