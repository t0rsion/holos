#!/usr/bin/env python3
"""Tables and stratum medians for the engineering benchmark.

Usage: engine_tables.py [--text] ENTRY_META TOTALS PHASES ARMS

benchmarks/engine_bench.sh writes three tab-separated records and calls this
script to turn them into markdown. Without --text the output is markdown.
With --text it is one key=value line per figure, for the .txt log. This
step computes medians and ratios. It measures nothing.

ENTRY_META  id, stratum, headline, competitor, n, edges, status
TOTALS      id, arm, config, median_s, iqr_s, rss_kb. The arm is an arm
            label, or "ripser" for the external arm, or "driver" for the
            single-repetition peak RSS probe of one engine entry point
PHASES      id, mode, phase, median_s
ARMS        space-separated label:commit:sha256:configurations items, in the
            order the record puts the columns

A void entry keeps its row, marked void, and enters no median.
"""
import sys

CONFIG_ORDER = [
    "auto",
    "forced-dense",
    "forced-sparse",
    "sparse-file",
    "dense",
    "sparse",
]
PHASE_ORDER = ["parse", "distance", "graph", "reduce", "total"]


def read_rows(path, width):
    rows = []
    with open(path) as f:
        for line in f:
            line = line.rstrip("\n")
            if not line:
                continue
            fields = line.split("\t")
            if len(fields) != width:
                sys.exit(f"{path}: expected {width} fields, got {len(fields)}: {line}")
            rows.append(fields)
    return rows


def number(text):
    """The value of a numeric cell, or None when the cell holds no number."""
    try:
        value = float(text)
    except (TypeError, ValueError):
        return None
    return value if value > 0.0 else None


def median(values):
    """The median, interpolated linearly between the two neighbouring order
    statistics, the rule benchmarks/_common.sh uses."""
    if not values:
        return None
    ordered = sorted(values)
    h = (len(ordered) - 1) * 0.5
    lo = int(h)
    if lo + 1 >= len(ordered):
        return ordered[-1]
    return ordered[lo] + (h - lo) * (ordered[lo + 1] - ordered[lo])


def config_key(name):
    return (CONFIG_ORDER.index(name) if name in CONFIG_ORDER else len(CONFIG_ORDER), name)


def seconds(value):
    return "n/a" if value is None else f"{value:.4f}"


def megabytes(kb):
    return "n/a" if kb is None else f"{kb / 1024:.1f}"


def as_ratio(value):
    return "n/a" if value is None else f"{value:.2f}"


def table(out, header, aligns, rows):
    out.append("| " + " | ".join(header) + " |")
    out.append("|" + "|".join(aligns) + "|")
    out.extend(rows)
    out.append("")


def main():
    argv = sys.argv[1:]
    as_text = False
    if argv and argv[0] == "--text":
        as_text = True
        argv = argv[1:]
    if len(argv) != 4:
        sys.exit("usage: engine_tables.py [--text] ENTRY_META TOTALS PHASES ARMS")
    meta_path, totals_path, phases_path, arms_text = argv

    entries = read_rows(meta_path, 7)
    totals = read_rows(totals_path, 6)
    phases = read_rows(phases_path, 4)
    arms = []
    for item in arms_text.split():
        label, commit, sha, state = item.split(":")
        arms.append((label, commit, sha, state))

    # value[(id, arm, config)] = (median_s, iqr_s, rss_kb)
    value = {}
    configs_of_arm = {}
    for entry_id, arm, config, med, iqr, rss in totals:
        value[(entry_id, arm, config)] = (number(med), number(iqr), number(rss))
        configs_of_arm.setdefault(arm, set()).add(config)

    columns = []
    for label, _, _, _ in arms:
        for config in sorted(configs_of_arm.get(label, ()), key=config_key):
            columns.append((label, config))
    ripser_configs = sorted(configs_of_arm.get("ripser", ()), key=config_key)
    driver_modes = sorted(configs_of_arm.get("driver", ()), key=config_key)

    out = []
    if not as_text:
        out.append("## Fresh-process totals")
        out.append("")
        out.append(
            "Seconds, median over the timed repetitions of one fresh process per"
        )
        out.append(
            "run. Edges are the pairs at or below the threshold, after the collapse"
        )
        out.append("where an entry collapses. A void entry lost a comparison and enters")
        out.append("no median below.")
        out.append("")
        header = ["id", "stratum", "headline", "n", "edges"]
        header += [f"{label} {config}" for label, config in columns]
        header += [f"ripser {config}" for config in ripser_configs]
        header.append("status")
        aligns = [":--", ":--", ":--", "--:", "--:"]
        aligns += ["--:"] * (len(columns) + len(ripser_configs))
        aligns.append(":--")
        rows = []
        for entry_id, stratum, headline, competitor, n, edges, status in entries:
            cells = [entry_id, stratum, headline, n, edges]
            for label, config in columns:
                cells.append(seconds(value.get((entry_id, label, config), (None,))[0]))
            for config in ripser_configs:
                cells.append(seconds(value.get((entry_id, "ripser", config), (None,))[0]))
            cells.append(status if status != "ok" else f"ok, {competitor}")
            rows.append("| " + " | ".join(cells) + " |")
        table(out, header, aligns, rows)

        out.append("## Ratios against ripser")
        out.append("")
        out.append("holos over ripser on the same file and the same configuration.")
        out.append("Below 1.0 means holos is faster. An entry whose competitor is none")
        out.append("has no external arm and no ratio. The routing configurations read")
        out.append("the entry's primary file; sparse-file has both tools read the")
        out.append("triplet file, so it is the like-for-like sparse-input ratio. On an")
        out.append("entry whose primary file is already the triplet file, sparse-file")
        out.append("repeats the auto measurement rather than time one command twice.")
        out.append("")
        header = ["id", "stratum"] + [f"{label} {config}" for label, config in columns]
        aligns = [":--", ":--"] + ["--:"] * len(columns)
        rows = []
        for entry_id, stratum, headline, competitor, n, edges, status in entries:
            cells = [entry_id, stratum]
            for label, config in columns:
                cells.append(as_ratio(ratio_of(value, entry_id, label, config, status)))
            rows.append("| " + " | ".join(cells) + " |")
        table(out, header, aligns, rows)

        out.append("## Peak RSS")
        out.append("")
        out.append("Megabytes. The arm and ripser figures are the largest VmHWM of")
        out.append("their timed runs. The driver columns come from one extra")
        out.append("single-repetition driver process per engine entry point, which is")
        out.append("the only way to give one entry point a peak of its own. The memory")
        out.append("stratum reads that pair.")
        out.append("")
        header = ["id", "stratum"]
        header += [f"{label} {config}" for label, config in columns]
        header += [f"ripser {config}" for config in ripser_configs]
        header += [f"driver {mode}" for mode in driver_modes]
        aligns = [":--", ":--"] + ["--:"] * (
            len(columns) + len(ripser_configs) + len(driver_modes)
        )
        rows = []
        for entry_id, stratum, headline, competitor, n, edges, status in entries:
            cells = [entry_id, stratum]
            for label, config in columns:
                cells.append(megabytes(value.get((entry_id, label, config), (None, None, None))[2]))
            for config in ripser_configs:
                cells.append(
                    megabytes(value.get((entry_id, "ripser", config), (None, None, None))[2])
                )
            for mode in driver_modes:
                cells.append(
                    megabytes(value.get((entry_id, "driver", mode), (None, None, None))[2])
                )
            rows.append("| " + " | ".join(cells) + " |")
        table(out, header, aligns, rows)

        out.append("## Driver phase medians")
        out.append("")
        out.append("Seconds, from the working tree's in-process driver, which no")
        out.append("historical arm has. Parse reads the input file. Distance builds")
        out.append("the full matrix (on a sparse input, the widening to +inf). Graph")
        out.append("builds the sparse graph. Reduce is the engine alone. The")
        out.append("reduction is one clock: the solver exposes no per-dimension")
        out.append("boundary outside the crate.")
        out.append("")
        by_entry = {}
        for entry_id, mode, phase, med in phases:
            by_entry.setdefault((entry_id, mode), {})[phase] = number(med)
        header = ["id", "engine entry point"] + PHASE_ORDER
        aligns = [":--", ":--"] + ["--:"] * len(PHASE_ORDER)
        rows = []
        for entry_id, mode in sorted(by_entry, key=lambda k: (order_of(entries, k[0]), k[1])):
            cells = [entry_id, mode]
            cells += [seconds(by_entry[(entry_id, mode)].get(p)) for p in PHASE_ORDER]
            rows.append("| " + " | ".join(cells) + " |")
        table(out, header, aligns, rows)

    strata = []
    for entry_id, stratum, headline, competitor, n, edges, status in entries:
        if stratum not in strata:
            strata.append(stratum)

    def stratum_median(name, label, config, headline_only=False):
        values = []
        for entry_id, stratum, headline, competitor, n, edges, status in entries:
            if status != "ok":
                continue
            if name is not None and stratum != name:
                continue
            if headline_only and headline != "yes":
                continue
            got = ratio_of(value, entry_id, label, config, status)
            if got is not None:
                values.append(got)
        return median(values), len(values)

    if as_text:
        for name in strata:
            for label, config in columns:
                med, count = stratum_median(name, label, config)
                print(
                    f"STRATUM stratum={name} arm={label} config={config} "
                    f"median_over_ripser={as_ratio(med)} entries={count}"
                )
        for label, config in columns:
            med, count = stratum_median(None, label, config)
            print(
                f"OVERALL arm={label} config={config} "
                f"median_over_ripser={as_ratio(med)} entries={count}"
            )
            med, count = stratum_median(None, label, config, headline_only=True)
            print(
                f"OVERALL headline_only arm={label} config={config} "
                f"median_over_ripser={as_ratio(med)} entries={count}"
            )
        return

    out.append("## Medians by stratum")
    out.append("")
    out.append("Median ratio against ripser inside each stratum, one column per arm")
    out.append("and configuration, with the number of entries that carried a ratio.")
    out.append("An overall median over unlike regimes hides the regime that lost.")
    out.append("")
    header = ["stratum", "entries"] + [f"{label} {config}" for label, config in columns]
    aligns = [":--", "--:"] + ["--:"] * len(columns)
    rows = []
    for name in strata:
        counted = 0
        cells = [name, ""]
        for label, config in columns:
            med, count = stratum_median(name, label, config)
            counted = max(counted, count)
            cells.append(as_ratio(med))
        cells[1] = str(counted)
        rows.append("| " + " | ".join(cells) + " |")
    table(out, header, aligns, rows)

    out.append("## Overall medians")
    out.append("")
    out.append("Read these only beside the table above.")
    out.append("")
    header = ["scope", "entries"] + [f"{label} {config}" for label, config in columns]
    aligns = [":--", "--:"] + ["--:"] * len(columns)
    rows = []
    for scope, headline_only in (("all entries", False), ("headline entries", True)):
        counted = 0
        cells = [scope, ""]
        for label, config in columns:
            med, count = stratum_median(None, label, config, headline_only)
            counted = max(counted, count)
            cells.append(as_ratio(med))
        cells[1] = str(counted)
        rows.append("| " + " | ".join(cells) + " |")
    table(out, header, aligns, rows)

    # The two tables above carry one entry count for a whole row, which is
    # the largest count over their columns. The sparse-file ratio is the
    # like-for-like comparison, so it gets its own count here.
    sparse_arms = [
        label for label, _, _, _ in arms if "sparse-file" in configs_of_arm.get(label, ())
    ]
    if sparse_arms:
        out.append("## Sparse-file comparison")
        out.append("")
        out.append("The sparse-file configuration alone, by stratum. holos and ripser")
        out.append("both read the triplet file, at the same threshold and the same")
        out.append("dimension, so this ratio compares like with like. The entries")
        out.append("column counts the entries of the stratum that carried the ratio.")
        out.append("The overall sparse-file median is the sparse-file column of the")
        out.append("table above.")
        out.append("")
        header = ["stratum"]
        for label in sparse_arms:
            header += [f"{label} sparse-file", f"{label} entries"]
        aligns = [":--"] + ["--:"] * (2 * len(sparse_arms))
        rows = []
        for name in strata:
            cells = [name]
            for label in sparse_arms:
                med, count = stratum_median(name, label, "sparse-file")
                cells += [as_ratio(med), str(count)]
            rows.append("| " + " | ".join(cells) + " |")
        table(out, header, aligns, rows)

    print("\n".join(out).rstrip() + "\n")


def order_of(entries, entry_id):
    for index, row in enumerate(entries):
        if row[0] == entry_id:
            return index
    return len(entries)


def ratio_of(value, entry_id, label, config, status):
    """holos over ripser for one entry, arm, and configuration."""
    if status != "ok":
        return None
    arm = value.get((entry_id, label, config))
    ripser = value.get((entry_id, "ripser", config))
    if not arm or not ripser or arm[0] is None or ripser[0] is None:
        return None
    return arm[0] / ripser[0]


if __name__ == "__main__":
    main()
