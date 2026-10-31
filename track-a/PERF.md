# Track A: performance log

Every step from the reference to your final version, each with the live-board run ID that measured it. Judges check that the run IDs exist and that each change moved the way you say it did.

| Step | Change | Run ID | T (s) | M (MB) | Points |
|---|---|---|---|---|---|
| 0 | The reference, unchanged | | 32.05 | 228 | 0 |
| 1 | Replace per-line Python date parsing with fixed-offset C++ parsing; intern endpoints and sort latency vectors only at output | pending board ID | pending | pending | pending |

Local `check.py` passed the official seed-1 full output hash
`495ca09682b9673ef836a506496177680bb4a3af54255174befddb1f83481cd3`.
The local Docker check took 1.11 s including container startup; this is not a
board measurement and is not used as a judging claim.

The seed-2 sample generated from the published generator also matched its Python
reference output hash `fd26778fa5e70fac2905a0a0f0142bd26ca4e6f510dfe182a881e525286ff12f`.
