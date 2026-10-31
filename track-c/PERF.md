# Track C: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 14.44 | 367 | 0 |
| 1 | Replace per-record Python objects with a mapped CSV parser, indexed join, and globally interned names with country membership bits | run-9f1d47d29d62 | 0.906522 | 12.34 | 42.51 |
| 2 | Store each distinct name in a compact open-addressed table, retaining bytewise equality checks | run-4bba7972a60f | 0.577201 | 9.91 | 48.01 |
| 3 | Parse three record-aligned ranges in parallel and merge name membership masks | pending board ID | pending | pending | pending |

Local `check.py` passed the official seed-1 full output hash
`d8088b1cd42ba1338b1345b107b55db7b96ef1f5066f3ff64fd26a8912a0f536`.
The local Docker check took 3.87 s including container startup; this is not a
board measurement and is not used as a judging claim.

Second local experiment: intern names globally and track country membership with
four 64-bit words per name. This replaces millions of per-country set nodes with
one state per name. The same full local sandbox check fell from 3.87 s to 2.15 s,
and the output hash stayed identical. A later board run, `run-f255ee6ba678`,
measured 0.770184 s and 12.52 MB with that implementation.

The compact table reduced the local full sandbox check from about 2.15 s to
1.73 s. The board measured 0.577201 s and 9.91 MB, raising K from 7.66 to 9.12.

The seed-2 sample generated from the published generator also matched its Python
reference output hash `f389938275df1b495a38c4375fe741795df180e64617e01e2cf77a95a56982a3`.
