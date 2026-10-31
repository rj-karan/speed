"""Independent aggregation oracle for hostile CSV and packed integer parsing."""
from pathlib import Path
import random
import subprocess
import tempfile
import unittest


class JoinCorrectness(unittest.TestCase):
    def test_quotes_money_hash_collisions_and_integer_lengths(self):
        rng = random.Random(31)
        names = ['', 'Betts', 'Betts ', 'Ann "Annie" Lee', 'Kaphar, Titus',
                 'abcdefghijklmnopAAAAAAAAqrstuvwx', 'abcdefghijklmnopBBBBBBBBqrstuvwx']
        names += ['A' * n + '"B' for n in [14, 15, 16, 29, 30, 31, 32, 61, 62, 63, 64]]
        names += ['Long name' + ' ' * 80]
        notes = ['', 'one\ntwo\nthree', 'comma, quote " together\nand a newline', 'trailing ']
        notes += ['"', '"no"', 'a' * 31 + '"' + 'b' * 32 + '"\nlast',
                  'a' * 63 + '"', '"' + 'a' * 64]
        regions = {1: 'AAA', 23: 'BBB', 456: 'CCC', 7890: 'DDD', 12345: 'AAA',
                   123456: 'BBB', 999999: 'CCC'}
        money_values = [-99999, -5, -1, 0, 1, 99, 100, 9999999]
        groups = {}
        records = []
        def money(cents):
            return ('-' if cents < 0 else '') + f'{abs(cents) // 100}.{abs(cents) % 100:02}'
        def quote(text, i):
            return '"' + text.replace('"', '""') + '"' if text or i % 2 else ''
        for i in range(2200):
            ident = [1, 12, 123, 1234, 12345, 123456, 1234567, 12345678][i] if i < 8 else 12345678 + i - 7
            name, note = rng.choice(names), rng.choice(notes)
            region = rng.choice(list(regions) + [2, 9999999])
            amount = rng.choice(money_values)
            key = regions.get(region, '_UNMATCHED')
            g = groups.setdefault(key, [0, 0, 0, amount, amount, set(), 0, 0])
            g[0] += 1; g[1] += ident; g[2] += amount
            g[3] = min(g[3], amount); g[4] = max(g[4], amount); g[5].add(name)
            g[6] += len(name.encode()); g[7] += len(note.encode())
            records.append(('\ufeff' if i and i % 31 == 0 else '') +
                           f'{ident},{quote(name, i)},{quote(note, i)},{region},{money(amount)}' +
                           ('\r\n' if i % 2 or i == 2199 else '\n'))
        expected = []
        for key, g in sorted(groups.items()):
            expected.append(' '.join([key, str(g[0]), str(g[1]), money(g[2]), money(g[3]),
                                      money(g[4]), str(len(g[5])), str(g[6]), str(g[7])]) + '\n')
        with tempfile.TemporaryDirectory(prefix='speed-join-') as tmp:
            root = Path(tmp); root.chmod(0o755)
            data, out = root / 'data', root / 'out'
            data.mkdir(); out.mkdir(); out.chmod(0o777)
            (data / 'regions.csv').write_text('region_id,country\n' + ''.join(f'{r},{c}\n' for r, c in sorted({**regions, 999998: 'EEE'}.items())))
            (data / 'records.csv').write_bytes(('\ufeffid,name,note,region_id,amount\r\n' + ''.join(records)).encode())
            subprocess.run(['docker', 'run', '--rm', '--network=none', '--read-only', '--cap-drop=ALL',
                            '--security-opt=no-new-privileges', '--user', '65534:65534',
                            '--memory=8g', '--memory-swap=8g', '--pids-limit=512',
                            '-v', str(data) + ':/data:ro', '-v', str(out) + ':/out:rw',
                            'speeddemon-check/track-c'], check=True, timeout=60)
            self.assertEqual(sorted(p.name for p in out.iterdir()), ['aggregates.txt'])
            self.assertEqual((out / 'aggregates.txt').read_bytes(), ''.join(expected).encode())
