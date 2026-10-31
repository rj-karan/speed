"""Differential Track B checks. Build first with check.py; no extra packages.

    python3 -m unittest discover -s tests -v
    SPEED_B_IMAGE=speed-local/baseline-b python3 -m unittest discover -s tests -v

Tests live outside track-b so validation code does not affect source-size scoring.
"""
import os
from pathlib import Path
import random
import re
import subprocess
import tempfile
import unittest


class IndexCorrectness(unittest.TestCase):
    def test_queries_against_byte_token_oracle(self):
        rng = random.Random(20261031)
        vocabulary = [b"a", b"and", b"or", b"not", b"bail", b"bond", b"007", b"7",
                      b"rich", b"z", b"stanbul", b"273", b"x" * 32]
        separators = [b" ", b"--", b"'", b". ", "—".encode(), "\u00a0".encode(), b"/", b"\xff"]
        texts = [b"", b"***", b"bail bail bond", b"BAIL BOND", b"bailbond",
                 "Zürich İSTANBUL 273K ﬁduciary".encode(), b"007 7",
                 b"a" * 31 + b" bail bond", b"bail " + b"x" * 32,
                 b"xbail bond bail xbond", b"and or not"]
        for _ in range(180):
            words = [rng.choice(vocabulary) for _ in range(rng.randrange(90))]
            texts.append(b"".join((w.upper() if rng.random() < .3 else w) + rng.choice(separators) for w in words))
        tokens = [re.findall(rb"[a-z0-9]+", text.lower()) for text in texts]
        vocab = sorted(set(vocabulary + [t for row in tokens for t in row] + [b"absent"]))
        postings = {w: {i for i, row in enumerate(tokens) if w in row} for w in vocab}
        queries, answers = [], []
        for case in range(2500):
            if case % 2:
                words = [rng.choice(vocab) for _ in range(rng.randint(2, 8))]
                if case % 5 == 0:
                    row = rng.choice([row for row in tokens if len(row) >= 2])
                    start = rng.randrange(len(row) - 1)
                    words = row[start:start + rng.randint(2, 8)]
                queries.append(b'PHRASE "' + b" ".join(words) + b'"')
                result = {i for i, row in enumerate(tokens)
                          if any(row[j:j + len(words)] == words for j in range(len(row) - len(words) + 1))}
            else:
                words = [rng.choice(vocab) for _ in range(rng.randint(1, 6))]
                fields = [words[0]]
                result = postings[words[0]].copy()
                for w in words[1:]:
                    op = rng.choice([b"AND", b"OR", b"NOT"])
                    fields.extend([op, w])
                    if op == b"AND": result &= postings[w]
                    elif op == b"OR": result |= postings[w]
                    else: result -= postings[w]
                queries.append(b"BOOL " + b" ".join(fields))
            ids = sorted(result)
            answers.append(" ".join(map(str, [len(ids), sum(ids)] + ids[:10])))
        with tempfile.TemporaryDirectory(prefix="speed-index-") as tmp:
            root = Path(tmp)
            root.chmod(0o755)
            data, out = root / "data", root / "out"
            data.mkdir(); out.mkdir(); out.chmod(0o777)
            (data / "docs.txt").write_bytes(b"".join(str(i).encode() + b"\t" + text + b"\n" for i, text in enumerate(texts)))
            (data / "queries.txt").write_bytes(b"\n".join(queries) + b"\n")
            subprocess.run(["docker", "run", "--rm", "--network=none", "--read-only",
                            "--cap-drop=ALL", "--security-opt=no-new-privileges", "--user", "65534:65534",
                            "--memory=8g", "--memory-swap=8g", "--pids-limit=512",
                            "-v", str(data) + ":/data:ro", "-v", str(out) + ":/out:rw",
                            os.environ.get("SPEED_B_IMAGE", "speeddemon-check/track-b")],
                           check=True, timeout=60)
            self.assertEqual(sorted(p.name for p in out.iterdir()), ["results.txt"])
            self.assertEqual((out / "results.txt").read_bytes(), ("\n".join(answers) + "\n").encode())


if __name__ == "__main__":
    unittest.main()
