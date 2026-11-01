# BeyondBug — Speed Demon 2026

Our entry for all four tracks of [Speed Demon](https://speeddemon.dev).
C++ implementations for log ingestion, search, CSV aggregation and graph traversal;
a tested Rust comparison is retained under experiments/.
All four solutions have zero non-standard-library runtime dependencies.

## Build and verify

Docker and Python 3 are required. Each track builds offline from allowlisted,
digest-pinned images; those base images must already be available locally.
Place the [official public datasets](https://speeddemon.dev/board/#datasets)
in `data/a` through `data/d`. Verify every downloaded input against the published
[SHA256SUMS.json](https://s3.amazonaws.com/raptors.dev/speeddemon.dev/2026/data/SHA256SUMS.json)
before running the output checks:

```sh
python3 check.py a --data data/a --expected 495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3
python3 check.py b --data data/b --expected f86973c210228141bcb6f1f1b10188ad2a9b65f71e7cd7cf4590b38de4e80927
python3 check.py c --data data/c --expected d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536
python3 check.py d --data data/d --expected a23dbdf0a3e49cc8901310a9ee1dc6a9a569a85297b6da75c2a7002cb2ed277e
python3 -m unittest discover -s tests -v
```

`check.py` verifies the output under the event's filesystem, privilege, memory
and network restrictions. Its elapsed time includes container startup; it is
not a board measurement. The differential tests use the built Track B image.

To test other seeds, clone the [official generators and references](https://github.com/hackathon-raptors/speed-demon-2026),
generate a separate dataset, run its Python reference and pass that output's
SHA-256 to `check.py`. Never substitute a public-seed hash for another seed.

## Approaches and evidence

| Track | Approach | Details |
|---|---|---|
| A | Hour-local parallel parsing, compact records and exact selection | [README](track-a/README.md), [PERF](track-a/PERF.md) |
| B | Exact hybrid postings/bitmaps and anchored phrase verification | [README](track-b/README.md), [PERF](track-b/PERF.md) |
| C | Parallel hostile CSV parsing, indexed region join, shared distinct-name masks | [README](track-c/README.md), [PERF](track-c/PERF.md) |
| D | BFS-ordered CSR, shared build counters, packed state and radix heaps | [README](track-d/README.md), [PERF](track-d/PERF.md) |

PERF logs distinguish official run IDs from local experiments and pending runs.
The board displays historical bests; final scoring evaluates the commit recorded
at freeze, so retaining a regression can hurt even when the displayed best stays
unchanged. The A cursor experiment was reverted on that evidence.

## Submission and scoring

Push this repository to **`speed-demon`**. Other branches do not trigger the
board. Keep the Speed Demon Board GitHub App installed through freeze and keep
the team form's repository/tracks/members current.

Code freeze is **November 2, 2026, 15:00 UTC (20:30 IST)**. The repository must be
public under an OSI-approved licence within one hour after freeze. This repository
uses MIT; preserve the organisers' attribution in [LICENSE](LICENSE).

The overall title uses the geometric mean of the best three track totals. Machine
performance contributes 65%; code quality, technique and the measured optimisation
log contribute 35%. Final scoring uses an unseen generator seed, five runs,
byte-identical outputs, and the kernel's peak cgroup memory. Local speedups and
the practice leaderboard do not establish a final placement.

The [complete official context](https://speeddemon.dev/speed-demon-context.md)
contains rules, scoring, track specifications, toolchain digests, clarifications,
and submission requirements. The frozen specs and harness take precedence.

## Dependencies

Submitted programs use the C++ standard library and Linux system calls;
all four manifests declare zero runtime package dependencies. The builds use
pinned GCC images and the runtimes use pinned Debian. The standalone Rust
comparison in [experiments/track-c-rust](experiments/track-c-rust) uses Rust's
standard library without crates and is outside the submitted track folders.

## Additional-seed checks

After building the four images with `check.py`, run independent full-scale seeds
against an official checkout. This checks each output repeatedly under the event
sandbox; it does not measure scoring performance:

```sh
python3 tools/validate_seeds.py --official /path/to/speed-demon-2026 --seeds 19 37 59 --repeats 20
```

Reports and inputs stay in ignored `data/holdouts/`; `--reuse` checks recorded
input/reference hashes before using an existing case. Set `--cpus` to the twelve
logical CPUs available locally. Repeated correct outputs test stability but do
not prove the absence of races; independent oracles and publication invariants
provide separate evidence.

## Next steps

Prioritize B's exact index construction and storage; benchmark time, peak memory
and canonical source together. A's remaining target is temporary memory, while C
needs more parser/lookup work to approach the leading times. D only contributes
to the overall score after overtaking another counted track. Before freeze,
repeat additional-seed checks on the submitted sources, confirm all official
gates, and retain measured improvements. See [PERF.md](PERF.md) for evidence.
