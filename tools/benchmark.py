#!/usr/bin/env python3
"""Interleave local process timings, checking every output against an exact hash.

Supply the official harness's statically compiled sd-timer. Docker startup is
outside its clock. These are local comparisons, never official score claims.
"""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import statistics
import subprocess
import tempfile


OUTPUTS = {"a": "summary.txt", "b": "results.txt",
           "c": "aggregates.txt", "d": "distances.txt"}


def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("track", choices=OUTPUTS)
    parser.add_argument("images", nargs="+", help="label=image pairs")
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--timer", type=Path, required=True)
    parser.add_argument("--expected", required=True, help="expected output SHA-256")
    parser.add_argument("--cpus", default="0-11")
    parser.add_argument("--repeats", type=int, default=5)
    parser.add_argument("--save", type=Path, required=True)
    args = parser.parse_args()
    if args.repeats < 3:
        parser.error("at least three repetitions are needed for a trimmed mean")
    images = {}
    for pair in args.images:
        label, separator, image = pair.partition("=")
        if not separator or not label or not image or label in images:
            parser.error("use distinct, nonempty label=image pairs")
        images[label] = subprocess.check_output(
            ["docker", "image", "inspect", "--format={{.Id}}", image],
            text=True).strip()
    data, timer = args.data.resolve(), args.timer.resolve()
    if not data.is_dir() or not timer.is_file():
        parser.error("--data must be a directory and --timer a static executable")
    # Hashing records the inputs and prewarms them before any measured process.
    inputs = {p.name: digest(p) for p in sorted(data.iterdir()) if p.is_file()}
    samples = {label: [] for label in images}
    with tempfile.TemporaryDirectory(prefix="speed-benchmark-") as temporary:
        root = Path(temporary)
        root.chmod(0o755)
        out, timing = root / "out", root / "timing"
        for folder in (out, timing):
            folder.mkdir()
            folder.chmod(0o777)
        result = out / OUTPUTS[args.track]
        clock = timing / "time.json"
        for repeat in range(args.repeats):
            for label, image in images.items():
                result.unlink(missing_ok=True)
                clock.unlink(missing_ok=True)
                subprocess.run([
                    "docker", "run", "--rm", "--network=none", "--read-only",
                    "--memory=8g", "--memory-swap=8g", "--pids-limit=512",
                    "--cap-drop=ALL", "--cap-add=SETUID", "--cap-add=SETGID",
                    "--security-opt=no-new-privileges", "--cpuset-cpus=" + args.cpus,
                    "--tmpfs", "/tmp:rw,size=1g,exec", "--ulimit", "nofile=8192:8192",
                    "-v", str(data) + ":/data:ro", "-v", str(out) + ":/out:rw",
                    "-v", str(timer) + ":/sd-timer:ro",
                    "-v", str(timing) + ":/timing:rw", "--entrypoint", "/sd-timer",
                    image, "/timing/time.json", "65534", "65534", "--", "/solution"
                ], check=True, timeout=300)
                if sorted(p.name for p in out.iterdir()) != [result.name]:
                    raise RuntimeError(f"{label}: extra or missing output files")
                if digest(result) != args.expected.lower():
                    raise RuntimeError(f"{label}: incorrect output SHA-256")
                seconds = json.loads(clock.read_text())["wall_ns"] / 1e9
                samples[label].append(seconds)
                print(f"{label} {repeat + 1}/{args.repeats}: {seconds:.6f} s", flush=True)
    means = {label: statistics.mean(sorted(values)[1:-1])
             for label, values in samples.items()}
    report = {"kind": "local process timing, not official scoring",
              "at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
              "track": args.track, "cpus": args.cpus, "images": images,
              "timer_sha256": digest(timer),
              "inputs_sha256": inputs, "expected_sha256": args.expected.lower(),
              "samples_s": samples, "trimmed_mean_s": means}
    args.save.parent.mkdir(parents=True, exist_ok=True)
    args.save.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(means, indent=2))


if __name__ == "__main__":
    main()
