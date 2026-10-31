#!/usr/bin/env python3
"""Compare built submissions with official references on fresh full-scale seeds.

This is a correctness/stability check, not a performance benchmark. Generation,
reference execution and Docker startup are deliberately outside score claims.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import sys


TASKS = {
    "a": ("a_ingest.py", "summary.txt"),
    "b": ("b_index.py", "results.txt"),
    "c": ("c_join.py", "aggregates.txt"),
    "d": ("d_traverse.py", "distances.txt"),
}


def digest(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--official", type=Path, required=True)
    parser.add_argument("--root", type=Path, default=Path("data/holdouts"))
    parser.add_argument("--seeds", type=int, nargs="+", default=[19, 37, 59])
    parser.add_argument("--tracks", default="abcd")
    parser.add_argument("--repeats", type=int, default=20)
    parser.add_argument("--cpus", default="0-11")
    parser.add_argument("--reuse", action="store_true",
                        help="reuse generated inputs and reference files after checking recorded hashes")
    args = parser.parse_args()
    if args.repeats < 1 or not args.tracks or any(t not in TASKS for t in args.tracks):
        parser.error("positive repeats and tracks chosen from abcd are required")
    official, root = args.official.resolve(), args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    images = {t: subprocess.check_output(
        ["docker", "image", "inspect", "--format={{.Id}}", "speeddemon-check/track-" + t],
        text=True).strip() for t in args.tracks}
    reports = []
    for seed in args.seeds:
        for track in args.tracks:
            script, output = TASKS[track]
            case = root / f"seed-{seed}" / track
            data, reference, out = case / "data", case / "reference", case / "out"
            for folder in (data, reference, out):
                folder.mkdir(parents=True, exist_ok=True)
            out.chmod(0o777)
            manifest = case / "verified.json"
            scripts = {name: digest(official / name / script)
                       for name in ("generators", "reference")}
            print(f"seed {seed} track {track}: prepare full-scale input/reference", flush=True)
            if args.reuse and manifest.exists():
                old = json.loads(manifest.read_text())
                assert old["scripts"] == scripts, "generator/reference changed; regenerate"
                assert old["inputs"] == {p.name: digest(p) for p in data.iterdir()}
                assert old["expected_sha256"] == digest(reference / output)
            else:
                subprocess.run([sys.executable, str(official / "generators" / script),
                                "--seed", str(seed), "--scale", "full", "--out", str(data)],
                               check=True, timeout=1200)
                subprocess.run([sys.executable, str(official / "reference" / script),
                                "--data", str(data), "--out", str(reference)],
                               check=True, timeout=1200)
            expected = digest(reference / output)
            report = {"seed": seed, "track": track, "scale": "full", "scripts": scripts,
                      "inputs": {p.name: digest(p) for p in data.iterdir()},
                      "expected_sha256": expected, "passed_repetitions": 0,
                      "cpus": args.cpus, "image": images[track]}
            for repeat in range(args.repeats):
                # Remove the previous result so a program that writes nothing cannot pass.
                (out / output).unlink(missing_ok=True)
                subprocess.run([
                    "docker", "run", "--rm", "--network=none", "--read-only",
                    "--memory=8g", "--memory-swap=8g", "--pids-limit=512",
                    "--cap-drop=ALL", "--security-opt=no-new-privileges",
                    "--user", "65534:65534", "--ulimit", "nofile=8192:8192",
                    "--cpuset-cpus=" + args.cpus, "--tmpfs", "/tmp:rw,size=1g,exec",
                    "-v", str(data) + ":/data:ro", "-v", str(out) + ":/out:rw",
                    report["image"]], check=True, timeout=300)
                assert sorted(p.name for p in out.iterdir()) == [output], "extra/missing output"
                assert digest(out / output) == expected, (seed, track, repeat, "wrong output")
                report["passed_repetitions"] += 1
                manifest.write_text(json.dumps(report, indent=2) + "\n")
                if (repeat + 1) % 5 == 0 or repeat + 1 == args.repeats:
                    print(f"seed {seed} track {track}: {repeat + 1}/{args.repeats} hashes match", flush=True)
            reports.append(report)
            (root / "report.json").write_text(json.dumps(reports, indent=2) + "\n")
    print(f"PASS: {len(reports)} full datasets, {len(reports) * args.repeats} sandbox runs", flush=True)


if __name__ == "__main__":
    main()
