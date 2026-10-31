# BeyondBug — Speed Demon 2026

Our entry for all four tracks of [Speed Demon](https://speeddemon.dev).
C++ implementations for log ingestion, search, CSV aggregation and graph traversal.
All four solutions have zero non-standard-library runtime dependencies.

## Build and verify

Docker and Python 3 are required. Each track builds offline from allowlisted,
digest-pinned images; those base images must already be available locally.
Place the [official public datasets](https://speeddemon.dev/board/#datasets)
in `data/a` through `data/d`, then run:

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
| A | Hour-local parallel parsing, compact records and nearest-rank selection | [README](track-a/README.md), [PERF](track-a/PERF.md) |
| B | Exact hybrid postings/bitmaps and anchored phrase verification | [README](track-b/README.md), [PERF](track-b/PERF.md) |
| C | Parallel hostile CSV parsing, indexed region join, shared distinct-name masks | [README](track-c/README.md), [PERF](track-c/PERF.md) |
| D | Forward/reverse CSR, radix heaps and four query workers | [README](track-d/README.md), [PERF](track-d/PERF.md) |

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
