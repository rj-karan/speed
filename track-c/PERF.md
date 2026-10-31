# Track C: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 14.44 | 367 | 0 |
| 1 | Replace per-record Python objects with a mapped CSV parser, indexed join, and globally interned names with country membership bits | run-9f1d47d29d62 | 0.906522 | 12.34 | 42.51 |
| 2 | Pure Rust rewrite (D=1): Flat 1M region array, RFC 4180 zero-copy CSV parser, open-addressing table with 256-bit country bitmasks | pending board ID | pending | pending | pending |
| 3 | 12-thread parallel CSV chunk parsing with safe trailing-decimal boundary detection, L1-resident thread accumulators, parallel distinct name reduction | pending board ID | pending | pending | pending |

Local `check.py` passed the official seed-1 full output hash
`d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536`.
The local Docker check took 0.37 s including container startup; this is not a
board measurement and is not used as a judging claim.
