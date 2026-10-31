"""Compare the sandboxed Track D binary with an independent heapq oracle."""
import heapq
import os
from pathlib import Path
import random
import subprocess
import tempfile
import unittest


class TraverseCorrectness(unittest.TestCase):
    def test_directed_paths_and_large_weights(self):
        rng = random.Random(917)
        n = 120
        edges = [(0, 1, 4294967295), (1, 2, 4294967295), (0, 1, 7),
                 (0, 1, 7), (2, 2, 0), (3, 4, 0), (4, 5, 0)]
        edges += [(rng.randrange(10, 110), rng.randrange(10, 110),
                   rng.choice([0, 1, 50, 999, 4294967295])) for _ in range(1100)]
        queries = [(i, j) for i in range(10) for j in range(10)]
        queries += [(rng.randrange(n), rng.randrange(n)) for _ in range(1000)]
        queries += queries[:50]
        adj = [[] for _ in range(n)]
        for u, v, w in edges: adj[u].append((v, w))
        expected = []
        for s, t in queries:
            distance = {s: 0}; queue = [(0, s)]; answer = -1
            while queue:
                d, u = heapq.heappop(queue)
                if d != distance[u]: continue
                if u == t: answer = d; break
                for v, w in adj[u]:
                    if d + w < distance.get(v, 2**64):
                        distance[v] = d + w; heapq.heappush(queue, (d + w, v))
            expected.append(str(answer))
        rng.shuffle(edges)
        with tempfile.TemporaryDirectory(prefix='speed-traverse-') as tmp:
            root = Path(tmp); root.chmod(0o755)
            data, out = root / 'data', root / 'out'
            data.mkdir(); out.mkdir(); out.chmod(0o777)
            (data / 'graph.txt').write_text(f'{n} {len(edges)}\n' + ''.join(f'{u} {v} {w}\n' for u, v, w in edges))
            (data / 'queries.txt').write_text(''.join(f'{s} {t}\n' for s, t in queries))
            subprocess.run(['docker', 'run', '--rm', '--network=none', '--read-only',
                            '--cap-drop=ALL', '--security-opt=no-new-privileges', '--user', '65534:65534',
                            '--memory=8g', '--memory-swap=8g', '--pids-limit=512',
                            '-v', str(data) + ':/data:ro', '-v', str(out) + ':/out:rw',
                            os.environ.get('SPEED_D_IMAGE', 'speeddemon-check/track-d')],
                           check=True, timeout=60)
            self.assertEqual(sorted(p.name for p in out.iterdir()), ['distances.txt'])
            self.assertEqual((out / 'distances.txt').read_bytes(), ('\n'.join(expected) + '\n').encode())
