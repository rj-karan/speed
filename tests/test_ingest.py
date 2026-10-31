"""Independent byte-output oracle for Track A; requires its built Docker image."""
import collections
import datetime
import os
from pathlib import Path
import random
import subprocess
import tempfile
import unittest


class IngestCorrectness(unittest.TestCase):
    def run_image(self, data):
        with tempfile.TemporaryDirectory(prefix='speed-ingest-') as tmp:
            root = Path(tmp); root.chmod(0o755)
            inputs, out = root / 'data', root / 'out'
            inputs.mkdir(); out.mkdir(); out.chmod(0o777)
            (inputs / 'requests.log').write_bytes(data)
            subprocess.run(['docker', 'run', '--rm', '--network=none', '--read-only',
                            '--cap-drop=ALL', '--security-opt=no-new-privileges',
                            '--user', '65534:65534', '--memory=8g', '--memory-swap=8g',
                            '--pids-limit=512', '-v', str(inputs) + ':/data:ro',
                            '-v', str(out) + ':/out:rw',
                            os.environ.get('SPEED_A_IMAGE', 'speeddemon-check/track-a')],
                           check=True, timeout=60)
            self.assertEqual(sorted(p.name for p in out.iterdir()), ['summary.txt'])
            return (out / 'summary.txt').read_bytes()

    def test_empty(self):
        self.assertEqual(self.run_image(b''), b'')

    def test_unordered_hours_percentiles_and_parser_boundaries(self):
        rng = random.Random(917)
        start = datetime.datetime(2026, 10, 30, 18)
        paths = ['/', '/health', '/v2/orders', '/v2/orders-export', '/v2/orders.csv',
                 '/v2/orders/{id}', '/v2/orders/{id}/items', '/v2/ordersByDate',
                 '/v2/orders_legacy', '/v2/orderstatus', '/' + 'x' * 124]
        paths += ['/' + 'q' * n for n in [6, 7, 8, 14, 15, 16, 23, 24, 25, 30, 31, 32, 63, 64]]
        methods = ['GET', 'POST', 'PUT', 'PATCH', 'DELETE', 'HEAD', 'OPTIONS']
        values = [0, 1, 9, 10, 99, 100, 999, 1000, 9999, 10000, 99999, 100000,
                  999999, 1000000, 9999999, 10000000, 99999999, 100000000,
                  999999999, 1000000000, 4294967295]
        groups = collections.defaultdict(lambda: [[], 0])
        lines = []
        for i in range(6000):
            h = rng.randrange(72); path = rng.choice(paths)
            timestamp = start + datetime.timedelta(hours=h, seconds=rng.randrange(3600))
            latency = rng.choice(values) if i % 2 else rng.randrange(2**32)
            status = rng.choice([100, 200, 499, 500, 599])
            row = f'{timestamp:%Y-%m-%dT%H:%M:%S}.000000Z {rng.choice(methods)} {path} {status} {latency}\n'
            repeat = 2 if i % 31 == 0 else 1
            lines.extend([row] * repeat)
            g = groups[(h, path)]; g[0].extend([latency] * repeat); g[1] += repeat * (status >= 500)
        rng.shuffle(lines)
        expected = []
        for (h, path), (latencies, errors) in sorted(groups.items()):
            latencies.sort(); n = len(latencies)
            quantiles = [latencies[(p * n + 99) // 100 - 1] for p in (50, 95, 99)]
            stamp = start + datetime.timedelta(hours=h)
            expected.append(f'{stamp:%Y-%m-%dT%H}:00:00Z {path} ' + ' '.join(map(str, [n, *quantiles, errors, errors * 1000000 // n])) + '\n')
        self.assertEqual(self.run_image(''.join(lines).encode()), ''.join(expected).encode())
