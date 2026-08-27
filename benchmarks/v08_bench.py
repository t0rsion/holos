#!/usr/bin/env python3
"""Run the registered version 0.8 point and factorization study.

Usage: v08_bench.py [--confirm]

The screen must finish before confirmation. Its manifest binds the commit,
both binaries, the corpus, and every generated screen input. Confirmation
refuses a missing or stale manifest.

Environment:
  CARGO             Cargo command, including an optional toolchain. The
                    default is ``cargo``.
  V07_BIN           Preserved version 0.7 holos binary. Required.
  REPS              Timed repetitions after one warm-up. The minimum and
                    default are 5.
  MEASURE_AFFINITY  CPUs for timed commands. The default is
                    ``0-3,12-15`` on the registered machine.
  ALLOW_DIRTY       Set to 1 to record a dirty study. Such a record is void
                    for release claims.

Point entries compare the preserved v0.7 point path with version 0.8 automatic
point construction. Block entries compare factorization off, automatic, and
forced. Every arm must produce the same diagram before a timing is retained.
"""

from __future__ import annotations

import argparse
import hashlib
import os
from pathlib import Path
import platform
import shlex
import statistics
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parent.parent
HERE = ROOT / "benchmarks"
DATA = HERE / "data"
CORPUS = HERE / "v08_corpus.toml"
MANIFEST = HERE / "results_v08_screen_manifest.txt"


def run_text(command, **kwargs):
    return subprocess.run(
        [str(item) for item in command],
        text=True,
        capture_output=True,
        check=True,
        **kwargs,
    ).stdout.strip()


def sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as source:
        for block in iter(lambda: source.read(1 << 20), b""):
            digest.update(block)
    return digest.hexdigest()


def percentile(values, fraction):
    values = sorted(values)
    position = (len(values) - 1) * fraction
    lower = int(position)
    upper = min(lower + 1, len(values) - 1)
    weight = position - lower
    return values[lower] * (1.0 - weight) + values[upper] * weight


def measure(command, stem, reps, affinity):
    output = DATA / f"v08_{stem}.out"
    error = DATA / f"v08_{stem}.err"
    environment = dict(os.environ)
    environment["MEASURE_AFFINITY"] = affinity
    environment["MEASURE_STDERR"] = str(error)
    measure_command = [sys.executable, HERE / "measure.py", output, *command]
    run_text(measure_command, env=environment)
    walls = []
    peak = 0
    for _ in range(reps):
        record = run_text(measure_command, env=environment)
        fields = dict(item.split("=", 1) for item in record.split())
        walls.append(float(fields["wall_s"]))
        peak = max(peak, int(fields["max_rss_kb"]))
    return {
        "median": statistics.median(walls),
        "q1": percentile(walls, 0.25),
        "q3": percentile(walls, 0.75),
        "min": min(walls),
        "max": max(walls),
        "rss_kb": peak,
        "output": output,
    }


def diagram(path):
    bars = []
    dimension = None
    with open(path) as source:
        for line in source:
            if line.startswith("persistence intervals in dim "):
                dimension = int(line.split()[4].rstrip(":"))
            elif line.startswith(" ["):
                birth, death = line.strip()[1:-1].split(",", 1)
                bars.append((dimension, birth, death))
    return sorted(bars)


def make_input(entry):
    DATA.mkdir(exist_ok=True)
    if entry["kind"] == "points":
        path = DATA / f"v08_{entry['id']}.csv"
        with open(path, "w") as output:
            subprocess.run(
                [
                    sys.executable,
                    HERE / "gen_cloud.py",
                    str(entry["n"]),
                    str(entry["coord_dim"]),
                    str(entry["seed"]),
                    entry["family"],
                ],
                text=True,
                stdout=output,
                check=True,
            )
        return path
    path = DATA / f"v08_{entry['id']}.sparse"
    with open(path, "w") as output:
        subprocess.run(
            [
                sys.executable,
                HERE / "gen_block_graph.py",
                str(entry["blocks"]),
                str(entry["block_size"]),
                str(entry["seed"]),
            ],
            text=True,
            stdout=output,
            check=True,
        )
    return path


def commands(entry, input_path, current, previous):
    common = [
        input_path,
        "--dim",
        entry["max_dim"],
        "--modulus",
        entry["modulus"],
        "--threads",
        entry["threads"],
    ]
    if entry["kind"] == "points":
        common += ["--threshold", entry["threshold"]]
        return {
            "v07": [previous, *common],
            "v08-auto": [current, *common],
        }
    common += ["--format", "sparse"]
    return {
        "off": [current, *common, "--factorization", "off"],
        "auto": [current, *common, "--factorization", "auto"],
        "force": [current, *common, "--factorization", "force"],
    }


def cpu_model():
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor() or "unknown"


def verify_screen(corpus_hash, commit, current_hash, previous_hash, bindings):
    if not MANIFEST.is_file():
        sys.exit("confirmation needs a complete screen manifest")
    fields = {}
    completed = {}
    for line in MANIFEST.read_text().splitlines():
        key, value = line.split("=", 1)
        if key == "entry":
            entry_id, input_hash = value.split(":", 1)
            completed[entry_id] = input_hash
        else:
            fields[key] = value
    expected = {
        "corpus_sha256": corpus_hash,
        "commit": commit,
        "current_sha256": current_hash,
        "previous_sha256": previous_hash,
    }
    if any(fields.get(key) != value for key, value in expected.items()) or completed != bindings:
        sys.exit("screen manifest is stale or incomplete")


def decisions(records):
    point = [record for record in records if record[0]["kind"] == "points"]
    blocks = [record for record in records if record[0]["kind"] == "blocks"]
    point_positive = bool(point) and all(
        ratio >= 1.5 and results["v08-auto"]["rss_kb"] < results["v07"]["rss_kb"]
        for _, _, _, results, ratio in point
    )
    factor_positive = bool(blocks) and all(
        ratio >= 1.25
        and results["auto"]["median"] <= 1.05 * results["force"]["median"]
        for _, _, _, results, ratio in blocks
    )
    regression = any(ratio < 0.95 for _, _, _, _, ratio in records)
    return point_positive, factor_positive, regression


def main():
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--confirm", action="store_true")
    parser.add_argument("-h", "--help", action="store_true")
    args = parser.parse_args()
    if args.help:
        print(__doc__)
        return

    reps = int(os.environ.get("REPS", "5"))
    if reps < 5:
        sys.exit("REPS must be at least 5")
    previous = os.environ.get("V07_BIN")
    if not previous:
        sys.exit("V07_BIN must name the preserved version 0.7 binary")
    previous = Path(previous).resolve()
    if not previous.is_file():
        sys.exit(f"V07_BIN does not exist: {previous}")
    cargo = os.environ.get("CARGO", "cargo").split()
    subprocess.run(
        [*cargo, "build", "--release", "-p", "holos-tda", "--locked"],
        cwd=ROOT,
        check=True,
    )
    current = ROOT / "target/release/holos"
    dirty = run_text(["git", "status", "--porcelain"], cwd=ROOT)
    if dirty and os.environ.get("ALLOW_DIRTY") != "1":
        sys.exit("worktree is dirty; commit first or set ALLOW_DIRTY=1")

    corpus_hash = sha256(CORPUS)
    with open(CORPUS, "rb") as source:
        corpus = tomllib.load(source)
    table = "confirm" if args.confirm else "screen"
    entries = corpus[table]
    affinity = os.environ.get("MEASURE_AFFINITY", "0-3,12-15")
    commit = run_text(["git", "rev-parse", "HEAD"], cwd=ROOT)
    if dirty:
        commit += "-DIRTY"
    current_hash = sha256(current)
    previous_hash = sha256(previous)
    screen_bindings = {
        entry["id"]: sha256(make_input(entry)) for entry in corpus["screen"]
    }
    if args.confirm:
        verify_screen(corpus_hash, commit, current_hash, previous_hash, screen_bindings)
    records = []
    for entry in entries:
        input_path = make_input(entry)
        arms = commands(entry, input_path, current, previous)
        results = {}
        reference = None
        for arm, command in arms.items():
            result = measure(command, f"{table}_{entry['id']}_{arm}", reps, affinity)
            got = diagram(result["output"])
            if reference is None:
                reference = got
            elif got != reference:
                sys.exit(f"{entry['id']}: {arm} diagram differs; timings are void")
            results[arm] = result
        baseline = results["v07" if entry["kind"] == "points" else "off"]["median"]
        preferred = results["v08-auto" if entry["kind"] == "points" else "auto"]["median"]
        records.append(
            (
                entry,
                sha256(input_path),
                arms,
                results,
                baseline / preferred if preferred else float("inf"),
            )
        )

    point_positive, factor_positive, regression = decisions(records)

    raw = HERE / f"results_v08_{table}.txt"
    markdown = HERE / f"results_v08_{table}.md"
    header = [
        f"commit={commit}",
        f"corpus={CORPUS.name}",
        f"corpus_version={corpus['meta']['version']}",
        f"corpus_sha256={corpus_hash}",
        f"set={table}",
        f"repetitions={reps}",
        f"affinity={affinity}",
        f"cpu={cpu_model()}",
        f"platform={platform.platform()}",
        f"python={platform.python_version()}",
        f"cargo={run_text([*cargo, '--version'])}",
        f"current_version={run_text([current, '--version'])}",
        f"current_sha256={current_hash}",
        f"previous_version={run_text([previous, '--version'])}",
        f"previous_sha256={previous_hash}",
        f"point_rule={'pass' if point_positive else 'fail'}",
        f"factorization_rule={'pass' if factor_positive else 'fail'}",
        f"preferred_regression={'yes' if regression else 'no'}",
    ]
    lines = list(header)
    for entry, input_hash, arms, results, ratio in records:
        lines.append(
            f"entry={entry['id']} kind={entry['kind']} input_sha256={input_hash} "
            f"preferred_speedup={ratio:.3f}"
        )
        for arm, result in results.items():
            lines.append(f"command={arm} {shlex.join(str(item) for item in arms[arm])}")
            lines.append(
                f"arm={arm} median_s={result['median']:.4f} q1_s={result['q1']:.4f} "
                f"q3_s={result['q3']:.4f} min_s={result['min']:.4f} "
                f"max_s={result['max']:.4f} max_rss_kb={result['rss_kb']}"
            )
    raw.write_text("\n".join(lines) + "\n")

    md = [
        "<!-- Generated by benchmarks/v08_bench.py. Do not edit. -->",
        "",
        f"- Commit: `{commit}`",
        f"- Corpus: `{CORPUS.name}` version {corpus['meta']['version']}, SHA-256 `{corpus_hash}`",
        f"- Set: {table}",
        f"- Repetitions: one warm-up and {reps} timed runs",
        f"- CPU affinity: `{affinity}`",
        f"- CPU: {cpu_model()}",
        f"- Platform: `{platform.platform()}`",
        f"- Python: `{platform.python_version()}`",
        f"- Cargo: `{run_text([*cargo, '--version'])}`",
        f"- Current binary: `{run_text([current, '--version'])}`, SHA-256 `{current_hash}`",
        f"- Control binary: `{run_text([previous, '--version'])}`, SHA-256 `{previous_hash}`",
        "- Diagram comparison: exact within each entry",
        f"- Point rule: {'pass' if point_positive else 'fail'}",
        f"- Factorization rule: {'pass' if factor_positive else 'fail'}",
        f"- Preferred-arm regression: {'yes' if regression else 'no'}",
        "",
        "| entry | kind | arm | median (s) | IQR (s) | peak RSS (MiB) | preferred speedup |",
        "|:--|:--|:--|--:|--:|--:|--:|",
    ]
    for entry, _, _, results, ratio in records:
        first = True
        for arm, result in results.items():
            speedup = f"{ratio:.2f}x" if first else ""
            md.append(
                f"| {entry['id']} | {entry['kind']} | {arm} | {result['median']:.4f} | "
                f"{result['q3'] - result['q1']:.4f} | {result['rss_kb'] / 1024:.1f} | "
                f"{speedup} |"
            )
            first = False
    markdown.write_text("\n".join(md) + "\n")
    if not args.confirm:
        MANIFEST.write_text(
            f"corpus_sha256={corpus_hash}\n"
            f"commit={commit}\n"
            f"current_sha256={current_hash}\n"
            f"previous_sha256={previous_hash}\n"
            + "".join(
                f"entry={entry_id}:{input_hash}\n"
                for entry_id, input_hash in screen_bindings.items()
            )
        )
    print(f"wrote {raw.relative_to(ROOT)} and {markdown.relative_to(ROOT)}")


if __name__ == "__main__":
    main()
