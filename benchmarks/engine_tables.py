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
    """The median, interpolated linearly between the two neighboring order
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


def parse_cli(argv):
    as_text = bool(argv and argv[0] == "--text")
    paths = argv[1:] if as_text else argv
    if len(paths) != 4:
        sys.exit("usage: engine_tables.py [--text] ENTRY_META TOTALS PHASES ARMS")
    return as_text, paths


def parse_arms(text):
    arms = []
    for item in text.split():
        label, commit, sha, state = item.split(":")
        arms.append((label, commit, sha, state))
    return arms


class Study:
    """The parsed records and derived columns of one engineering study."""

    def __init__(self, entries, totals, phases, arms):
        self.entries = entries
        self.phases = phases
        self.arms = arms
        self.value = {}
        self.configs_of_arm = {}
        for entry_id, arm, config, med, iqr, rss in totals:
            self.value[(entry_id, arm, config)] = (
                number(med),
                number(iqr),
                number(rss),
            )
            self.configs_of_arm.setdefault(arm, set()).add(config)
        self.columns = []
        for label, _, _, _ in arms:
            configs = sorted(self.configs_of_arm.get(label, ()), key=config_key)
            self.columns.extend((label, config) for config in configs)
        self.ripser_configs = sorted(
            self.configs_of_arm.get("ripser", ()), key=config_key
        )
        self.driver_modes = sorted(
            self.configs_of_arm.get("driver", ()), key=config_key
        )

    def timing(self, entry_id, arm, config):
        return self.value.get((entry_id, arm, config), (None,))[0]

    def rss(self, entry_id, arm, config):
        return self.value.get((entry_id, arm, config), (None, None, None))[2]

    def ratio(self, entry_id, label, config, status):
        return ratio_of(self.value, entry_id, label, config, status)

    def strata(self):
        names = []
        for row in self.entries:
            if row[1] not in names:
                names.append(row[1])
        return names

    def stratum_median(self, name, label, config, headline_only=False):
        values = []
        for entry_id, stratum, headline, _, _, _, status in self.entries:
            if status != "ok" or (name is not None and stratum != name):
                continue
            if headline_only and headline != "yes":
                continue
            got = self.ratio(entry_id, label, config, status)
            if got is not None:
                values.append(got)
        return median(values), len(values)


def load_study(paths):
    meta_path, totals_path, phases_path, arms_text = paths
    return Study(
        read_rows(meta_path, 7),
        read_rows(totals_path, 6),
        read_rows(phases_path, 4),
        parse_arms(arms_text),
    )


def section(out, title, paragraphs):
    out.append(f"## {title}")
    out.append("")
    out.extend(paragraphs)
    out.append("")


def fresh_process_rows(study):
    rows = []
    for entry_id, stratum, headline, competitor, n, edges, status in study.entries:
        cells = [entry_id, stratum, headline, n, edges]
        for label, config in study.columns:
            cells.append(seconds(study.timing(entry_id, label, config)))
        for config in study.ripser_configs:
            cells.append(seconds(study.timing(entry_id, "ripser", config)))
        cells.append(status if status != "ok" else f"ok, {competitor}")
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_fresh_process(out, study):
    section(
        out,
        "Fresh-process totals",
        [
            "Seconds, median over the timed repetitions of one fresh process per",
            "run. Edges are the pairs at or below the threshold, after the collapse",
            "where an entry collapses. A void entry lost a comparison and enters",
            "no median below.",
        ],
    )
    header = ["id", "stratum", "headline", "n", "edges"]
    header += [f"{label} {config}" for label, config in study.columns]
    header += [f"ripser {config}" for config in study.ripser_configs]
    header.append("status")
    numeric = len(study.columns) + len(study.ripser_configs)
    aligns = [":--", ":--", ":--", "--:", "--:"] + ["--:"] * numeric
    aligns.append(":--")
    table(out, header, aligns, fresh_process_rows(study))


def ratio_rows(study):
    rows = []
    for entry_id, stratum, _, _, _, _, status in study.entries:
        cells = [entry_id, stratum]
        for label, config in study.columns:
            cells.append(as_ratio(study.ratio(entry_id, label, config, status)))
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_ratios(out, study):
    section(
        out,
        "Ratios against ripser",
        [
            "holos over ripser on the same file and the same configuration.",
            "Below 1.0 means holos is faster. An entry whose competitor is none",
            "has no external arm and no ratio. The routing configurations read",
            "the entry's primary file; sparse-file has both tools read the",
            "triplet file, so it is the like-for-like sparse-input ratio. On an",
            "entry whose primary file is already the triplet file, sparse-file",
            "repeats the auto measurement rather than time one command twice.",
        ],
    )
    header = ["id", "stratum"] + [
        f"{label} {config}" for label, config in study.columns
    ]
    aligns = [":--", ":--"] + ["--:"] * len(study.columns)
    table(out, header, aligns, ratio_rows(study))


def rss_rows(study):
    rows = []
    for entry_id, stratum, _, _, _, _, _ in study.entries:
        cells = [entry_id, stratum]
        for label, config in study.columns:
            cells.append(megabytes(study.rss(entry_id, label, config)))
        for config in study.ripser_configs:
            cells.append(megabytes(study.rss(entry_id, "ripser", config)))
        for mode in study.driver_modes:
            cells.append(megabytes(study.rss(entry_id, "driver", mode)))
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_rss(out, study):
    section(
        out,
        "Peak RSS",
        [
            "Megabytes. The arm and ripser figures are the largest VmHWM of",
            "their timed runs. The driver columns come from one extra",
            "single-repetition driver process per engine entry point, which is",
            "the only way to give one entry point a peak of its own. The memory",
            "stratum reads that pair.",
        ],
    )
    header = ["id", "stratum"]
    header += [f"{label} {config}" for label, config in study.columns]
    header += [f"ripser {config}" for config in study.ripser_configs]
    header += [f"driver {mode}" for mode in study.driver_modes]
    numeric = len(study.columns) + len(study.ripser_configs) + len(study.driver_modes)
    aligns = [":--", ":--"] + ["--:"] * numeric
    table(out, header, aligns, rss_rows(study))


def phase_rows(study):
    by_entry = {}
    for entry_id, mode, phase, med in study.phases:
        by_entry.setdefault((entry_id, mode), {})[phase] = number(med)
    keys = sorted(
        by_entry,
        key=lambda key: (order_of(study.entries, key[0]), key[1]),
    )
    rows = []
    for entry_id, mode in keys:
        cells = [entry_id, mode]
        cells += [seconds(by_entry[(entry_id, mode)].get(phase)) for phase in PHASE_ORDER]
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_phases(out, study):
    section(
        out,
        "Driver phase medians",
        [
            "Seconds, from the working tree's in-process driver, which no",
            "historical arm has. Parse reads the input file. Distance builds",
            "the full matrix (on a sparse input, the widening to +inf). Graph",
            "builds the sparse graph. Reduce is the engine alone. The",
            "reduction is one clock: the solver exposes no per-dimension",
            "boundary outside the crate.",
        ],
    )
    header = ["id", "engine entry point"] + PHASE_ORDER
    aligns = [":--", ":--"] + ["--:"] * len(PHASE_ORDER)
    table(out, header, aligns, phase_rows(study))


def print_text(study):
    for name in study.strata():
        for label, config in study.columns:
            med, count = study.stratum_median(name, label, config)
            print(
                f"STRATUM stratum={name} arm={label} config={config} "
                f"median_over_ripser={as_ratio(med)} entries={count}"
            )
    for label, config in study.columns:
        med, count = study.stratum_median(None, label, config)
        print(
            f"OVERALL arm={label} config={config} "
            f"median_over_ripser={as_ratio(med)} entries={count}"
        )
        med, count = study.stratum_median(None, label, config, headline_only=True)
        print(
            f"OVERALL headline_only arm={label} config={config} "
            f"median_over_ripser={as_ratio(med)} entries={count}"
        )


def stratum_rows(study):
    rows = []
    for name in study.strata():
        counted = 0
        cells = [name, ""]
        for label, config in study.columns:
            med, count = study.stratum_median(name, label, config)
            counted = max(counted, count)
            cells.append(as_ratio(med))
        cells[1] = str(counted)
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_stratum_medians(out, study):
    section(
        out,
        "Medians by stratum",
        [
            "Median ratio against ripser inside each stratum, one column per arm",
            "and configuration, with the number of entries that carried a ratio.",
            "An overall median over unlike regimes hides the regime that lost.",
        ],
    )
    header = ["stratum", "entries"] + [
        f"{label} {config}" for label, config in study.columns
    ]
    aligns = [":--", "--:"] + ["--:"] * len(study.columns)
    table(out, header, aligns, stratum_rows(study))


def overall_rows(study):
    rows = []
    for scope, headline_only in (("all entries", False), ("headline entries", True)):
        counted = 0
        cells = [scope, ""]
        for label, config in study.columns:
            med, count = study.stratum_median(None, label, config, headline_only)
            counted = max(counted, count)
            cells.append(as_ratio(med))
        cells[1] = str(counted)
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_overall_medians(out, study):
    section(out, "Overall medians", ["Read these only beside the table above."])
    header = ["scope", "entries"] + [
        f"{label} {config}" for label, config in study.columns
    ]
    aligns = [":--", "--:"] + ["--:"] * len(study.columns)
    table(out, header, aligns, overall_rows(study))


def sparse_file_rows(study, sparse_arms):
    rows = []
    for name in study.strata():
        cells = [name]
        for label in sparse_arms:
            med, count = study.stratum_median(name, label, "sparse-file")
            cells += [as_ratio(med), str(count)]
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_sparse_file(out, study):
    sparse_arms = [
        label
        for label, _, _, _ in study.arms
        if "sparse-file" in study.configs_of_arm.get(label, ())
    ]
    if not sparse_arms:
        return
    section(
        out,
        "Sparse-file comparison",
        [
            "The sparse-file configuration alone, by stratum. holos and ripser",
            "both read the triplet file, at the same threshold and the same",
            "dimension, so this ratio compares like with like. The entries",
            "column counts the entries of the stratum that carried the ratio.",
            "The overall sparse-file median is the sparse-file column of the",
            "table above.",
        ],
    )
    header = ["stratum"]
    for label in sparse_arms:
        header += [f"{label} sparse-file", f"{label} entries"]
    aligns = [":--"] + ["--:"] * (2 * len(sparse_arms))
    table(out, header, aligns, sparse_file_rows(study, sparse_arms))


def markdown(study):
    out = []
    append_fresh_process(out, study)
    append_ratios(out, study)
    append_rss(out, study)
    append_phases(out, study)
    append_stratum_medians(out, study)
    append_overall_medians(out, study)
    append_sparse_file(out, study)
    return "\n".join(out).rstrip() + "\n"


def main():
    as_text, paths = parse_cli(sys.argv[1:])
    study = load_study(paths)
    if as_text:
        print_text(study)
    else:
        print(markdown(study))


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
