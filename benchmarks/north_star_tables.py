#!/usr/bin/env python3
"""Tables and grades for the registered north-star study.

Usage: north_star_tables.py [--text] [options] ENTRY_META TOTALS SCALING_META ARMS

benchmarks/north_star.sh writes three tab-separated records and calls this
script to turn them into markdown. Without --text the output is markdown.
With --text it is one key=value line per figure, for the .txt log. This
step computes medians, ratios, the noise band, and the grades. It measures
nothing.

ENTRY_META    id, input stratum, engine selected, max_dim, n, edges, headline,
              competitor, status
TOTALS        id, pass, arm, threads, median_s, iqr_s, rss_kb. The arm is an
              arm label, "h3aa" for the A/A control, a ripser build name, or
              "gph" for giotto-ph
SCALING_META  id, engine selected, max_dim, n, edges, status
ARMS          space-separated label:commit:sha256 items, in record order

Options:
  --short-run S       the short-run floor in seconds (default 0.02). A ratio
                      whose denominator median is under it is descriptive:
                      the tables report it, and it enters no median, no
                      per-entry clause, no regression aggregate, and no A/A
                      band
  --slack X           the per-entry allowance of the decision rule
                      (default 0.05)
  --band-percentile P the percentile of the A/A distances from 1.0 that
                      defines the noise band (default 95)
  --physical-cores N  the core count of the multicore pass (default 4)

A void entry keeps its row, marked void, and enters no median and no grade.
A descriptive entry keeps its row and its ratio, marked descriptive, and
enters no median either. Amendment 1 of the corpus sets both rules.
"""
import sys

GRADING_STRATA = ("dense-selected", "sparse-selected", "maxdim-1", "maxdim-2")


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


def quantile(values, p):
    """The p quantile, 0 <= p <= 1, interpolated linearly between the two
    neighbouring order statistics, the rule benchmarks/_common.sh uses."""
    if not values:
        return None
    ordered = sorted(values)
    h = (len(ordered) - 1) * p
    lo = int(h)
    if lo + 1 >= len(ordered):
        return ordered[-1]
    return ordered[lo] + (h - lo) * (ordered[lo + 1] - ordered[lo])


def median(values):
    return quantile(values, 0.5)


def seconds(value):
    return "n/a" if value is None else f"{value:.4f}"


def megabytes(kb):
    return "n/a" if kb is None else f"{kb / 1024:.1f}"


def as_ratio(value):
    return "n/a" if value is None else f"{value:.3f}"


def table(out, header, aligns, rows):
    out.append("| " + " | ".join(header) + " |")
    out.append("|" + "|".join(aligns) + "|")
    out.extend(rows if rows else ["| " + " | ".join(["n/a"] * len(header)) + " |"])
    out.append("")


class Study:
    """One record: the entries, their timings, and every figure read from
    them. Nothing here measures; every number comes from TOTALS."""

    def __init__(self, entries, totals, scaling, arms, options):
        self.entries = entries
        self.scaling = scaling
        self.arms = arms
        self.options = options
        self.value = {}
        for entry_id, pass_name, arm, threads, med, iqr, rss in totals:
            self.value[(entry_id, pass_name, arm, threads)] = (
                number(med), number(iqr), number(rss)
            )
        self.holos_arms = [label for label, _, _ in arms if label != "h3aa"]
        self.earlier_arms = [label for label in self.holos_arms if label != "h3"]

    def wall(self, entry_id, pass_name, arm, threads):
        got = self.value.get((entry_id, pass_name, arm, str(threads)))
        return got[0] if got else None

    def rss(self, entry_id, pass_name, arm, threads):
        got = self.value.get((entry_id, pass_name, arm, str(threads)))
        return got[2] if got else None

    def ok_entries(self):
        return [row for row in self.entries if row[8] == "ok"]

    def competitor_of(self, row):
        return row[7]

    def graded(self, row, pass_name, denominator, threads):
        """True when a ratio over this denominator grades the entry: the entry
        kept its agreement, the denominator ran, and its median is at or above
        the short-run floor. Under the floor the ratio is descriptive, and it
        enters no median, no per-entry clause, no regression aggregate, and no
        band."""
        if row[8] != "ok":
            return False
        base = self.wall(row[0], pass_name, denominator, threads)
        return base is not None and base >= self.options["short_run"]

    def ratio(self, entry_id, pass_name, arm, competitor, threads):
        top = self.wall(entry_id, pass_name, arm, threads)
        base = self.wall(entry_id, pass_name, competitor, threads)
        if top is None or base is None:
            return None
        return top / base

    def regression_base(self, entry_id, pass_name, threads):
        """The fastest earlier arm on one entry. It is the denominator of the
        regression ratio, so the short-run floor reads it."""
        earlier = [
            self.wall(entry_id, pass_name, arm, threads) for arm in self.earlier_arms
        ]
        earlier = [w for w in earlier if w is not None]
        return min(earlier) if earlier else None

    def regression_ratio(self, entry_id, pass_name, threads):
        """h3 over the fastest earlier arm on the same entry."""
        top = self.wall(entry_id, pass_name, "h3", threads)
        base = self.regression_base(entry_id, pass_name, threads)
        if top is None or base is None:
            return None
        return top / base

    def regression_graded(self, row, pass_name, threads):
        """True when the regression clause grades this entry: its fastest
        earlier arm is at or above the short-run floor."""
        if row[8] != "ok":
            return False
        base = self.regression_base(row[0], pass_name, threads)
        return base is not None and base >= self.options["short_run"]

    def band(self, pass_name, threads, graded_of):
        """The noise band of one pass and the number of entries behind it: the
        chosen percentile of the absolute distance from 1.0 of the A/A control
        ratios, over the graded entries of the pass. An A/A ratio divides by
        the h3 median, so an entry whose h3 median is under the short-run floor
        enters no band; an entry the pass does not grade enters none either,
        because the band is what grades an entry."""
        distances = []
        for row in self.ok_entries():
            if not graded_of(row) or not self.graded(row, pass_name, "h3", threads):
                continue
            got = self.ratio(row[0], pass_name, "h3aa", "h3", threads)
            if got is not None:
                distances.append(abs(got - 1.0))
        if not distances:
            return None, 0
        return (
            quantile(distances, self.options["band_percentile"] / 100.0),
            len(distances),
        )

    def entry_limit(self, band):
        slack = 1.0 + self.options["slack"]
        return slack if band is None else max(slack, 1.0 + band)

    def strata_of(self, row):
        """The grading strata one entry belongs to. Every entry has a routing
        stratum; only max_dim 1 and 2 have a dimension stratum."""
        names = [row[2]]
        if row[3] in ("1", "2"):
            names.append(f"maxdim-{row[3]}")
        return names

    def input_strata(self):
        seen = []
        for row in self.entries:
            if row[1] not in seen:
                seen.append(row[1])
        return seen


def stratum_median(study, rows, ratio_of, name=None, headline_only=False,
                   graded_of=None):
    """The median ratio inside one stratum, the number of graded entries behind
    it, and the number of descriptive entries it left out. With no graded_of
    every entry that carries a ratio counts."""
    values = []
    descriptive = 0
    for row in rows:
        if name is not None and name not in study.strata_of(row):
            continue
        if headline_only and row[6] != "yes":
            continue
        got = ratio_of(row)
        if got is None:
            continue
        if graded_of is not None and not graded_of(row):
            descriptive += 1
            continue
        values.append(got)
    return median(values), len(values), descriptive


def grade_pass(study, pass_name, threads, competitor_of, arm="h3"):
    """Every figure the decision rule reads for one pass, in one dict. Every
    median here is taken over the graded entries alone."""
    rows = [row for row in study.ok_entries() if study.wall(row[0], pass_name, arm, threads)]

    def graded_of(row):
        return study.graded(row, pass_name, competitor_of(row), threads)

    band, band_entries = study.band(pass_name, threads, graded_of)
    limit = study.entry_limit(band)

    def ratio_of(row):
        return study.ratio(row[0], pass_name, arm, competitor_of(row), threads)

    def regression_graded_of(row):
        return study.regression_graded(row, pass_name, threads)

    def regression_of(row):
        return study.regression_ratio(row[0], pass_name, threads)

    strata = {}
    for name in GRADING_STRATA:
        strata[name] = stratum_median(study, rows, ratio_of, name, graded_of=graded_of)
    overall = stratum_median(
        study, rows, ratio_of, None, headline_only=True, graded_of=graded_of
    )

    violations = []
    for row in rows:
        if not graded_of(row):
            continue
        got = ratio_of(row)
        if got is not None and got > limit:
            violations.append((row[0], got))

    regression_strata = {}
    regression_violations = []
    if study.earlier_arms:
        for name in GRADING_STRATA:
            regression_strata[name] = stratum_median(
                study, rows, regression_of, name, graded_of=regression_graded_of
            )
        for row in rows:
            if not regression_graded_of(row):
                continue
            got = regression_of(row)
            if got is not None and got > limit:
                regression_violations.append((row[0], got))

    descriptive = [row[0] for row in rows if not graded_of(row)]
    return {
        "pass": pass_name,
        "threads": threads,
        "entries": len(rows),
        "graded": len(rows) - len(descriptive),
        "band": band,
        "band_entries": band_entries,
        "limit": limit,
        "strata": strata,
        "overall": overall,
        "violations": violations,
        "regression_strata": regression_strata,
        "regression_violations": regression_violations,
        "descriptive": descriptive,
        "graded_of": graded_of,
        "regression_graded_of": regression_graded_of,
    }


def clauses(grade, with_regression=True, competitor="ripser"):
    """The frozen clauses of one pass, each with its verdict and the numbers
    behind it."""
    out = []
    for name in GRADING_STRATA:
        med, count, descriptive = grade["strata"].get(name, (None, 0, 0))
        out.append((
            f"stratum {name} at or below {competitor}",
            None if med is None else med <= 1.0,
            f"median {as_ratio(med)} over {count} graded entries"
            + (f", {descriptive} descriptive" if descriptive else ""),
        ))
    med, count, descriptive = grade["overall"]
    out.append((
        f"overall median at or below {competitor}, headline entries",
        None if med is None else med <= 1.0,
        f"median {as_ratio(med)} over {count} graded entries"
        + (f", {descriptive} descriptive" if descriptive else ""),
    ))
    limit = grade["limit"]
    band = grade["band"]
    detail = (
        f"limit {as_ratio(limit)} (5% or the band {as_ratio(band)}, whichever is "
        f"wider) over {grade['graded']} graded entries of {grade['entries']}"
    )
    if grade["violations"]:
        detail += "; over it: " + ", ".join(
            f"{name} {as_ratio(value)}" for name, value in grade["violations"]
        )
    passed = None if grade["graded"] == 0 else not grade["violations"]
    out.append(("no entry above the per-entry limit", passed, detail))
    if with_regression and grade["regression_strata"]:
        worst = max(
            (med for med, _, _ in grade["regression_strata"].values() if med is not None),
            default=None,
        )
        detail = "worst stratum median " + as_ratio(worst) + "; " + ", ".join(
            f"{name} {as_ratio(med)} over {count} graded"
            for name, (med, count, _) in grade["regression_strata"].items()
        )
        if grade["regression_violations"]:
            detail += "; entries over the limit: " + ", ".join(
                f"{name} {as_ratio(value)}"
                for name, value in grade["regression_violations"]
            )
        out.append((
            "no material regression against the best earlier arm",
            None if worst is None else (worst <= 1.05 and not grade["regression_violations"]),
            detail,
        ))
    return out


def standing(study, row, grade):
    """How one entry stands in one pass: void, descriptive, or graded."""
    if row[8] != "ok":
        return "void"
    return "yes" if grade["graded_of"](row) else "descriptive"


def verdict(passed):
    if passed is None:
        return "not run"
    return "pass" if passed else "FAIL"


def markdown(study, serial, multicore):
    out = []
    cores = study.options["physical_cores"]
    serial_arms = [label for label, _, _ in study.arms]
    competitors = sorted(
        {
            arm
            for (_, pass_name, arm, threads) in study.value
            if pass_name == "serial" and arm not in serial_arms
        }
    )
    stock = [c for c in competitors if not c.endswith("-f64")]
    f64 = [c for c in competitors if c.endswith("-f64")]

    out.append("## Entries")
    out.append("")
    out.append("The engine column is the engine the frozen routing rule of H3 selects")
    out.append("for the entry, computed from the point count and the edge count at the")
    out.append("threshold. It is a property of the input and the frozen constants, not")
    out.append("of a measurement. A void entry lost its agreement pass and enters no")
    out.append("median below.")
    out.append("")
    table(
        out,
        ["id", "input stratum", "engine", "max dim", "n", "edges", "headline", "competitor", "status"],
        [":--", ":--", ":--", "--:", "--:", "--:", ":--", ":--", ":--"],
        ["| " + " | ".join(row) + " |" for row in study.entries],
    )

    out.append("## Serial fresh-process totals")
    out.append("")
    out.append("Seconds on one physical core, median over the timed repetitions of one")
    out.append("fresh process per run. `h3aa` is the A/A control: the same bytes as")
    out.append("`h3`, timed as its own arm.")
    out.append("")
    columns = serial_arms + stock + f64
    table(
        out,
        ["id"] + columns,
        [":--"] + ["--:"] * len(columns),
        [
            "| " + " | ".join(
                [row[0]] + [seconds(study.wall(row[0], "serial", arm, 1)) for arm in columns]
            ) + " |"
            for row in study.entries
        ],
    )

    out.append("## Serial ratios against ripser")
    out.append("")
    out.append("Each holos arm over the entry's own ripser build, on the same file at")
    out.append("the same threshold. Below 1.000 means holos is faster. The A/A column")
    out.append("is the control ratio against `h3`, not against ripser: it is what the")
    out.append("harness reports for two identical binaries.")
    out.append("")
    out.append("The graded column is `descriptive` when the entry's ripser median is")
    out.append(f"under the short-run floor of {study.options['short_run']:.3f} s. Such a")
    out.append("row is reported, and it enters no median, no per-entry clause, no")
    out.append("regression aggregate, and no band.")
    out.append("")
    header = (["id", "engine", "max dim"]
              + [a for a in serial_arms if a != "h3aa"]
              + ["h3aa over h3", "graded"])
    rows = []
    for row in study.entries:
        cells = [row[0], row[2], row[3]]
        for arm in [a for a in serial_arms if a != "h3aa"]:
            cells.append(as_ratio(study.ratio(row[0], "serial", arm, study.competitor_of(row), 1)))
        cells.append(as_ratio(study.ratio(row[0], "serial", "h3aa", "h3", 1)))
        cells.append(standing(study, row, serial))
        rows.append("| " + " | ".join(cells) + " |")
    table(out, header,
          [":--", ":--", "--:"] + ["--:"] * (len(header) - 4) + [":--"], rows)

    out.append("## Medians by input stratum")
    out.append("")
    out.append("Median serial ratio inside each input stratum, one column per holos")
    out.append("arm, with the number of graded entries behind the median and the")
    out.append("number of descriptive entries it left out. A median over unlike")
    out.append("regimes hides the regime that lost.")
    out.append("")
    arms_only = [a for a in serial_arms if a != "h3aa"]
    header = ["input stratum", "graded entries", "descriptive"] + arms_only
    rows = []
    for name in study.input_strata():
        subset = [row for row in study.ok_entries() if row[1] == name]
        cells = [name, "", ""]
        counted, left_out = 0, 0
        for arm in arms_only:
            med, count, descriptive = stratum_median(
                study,
                subset,
                lambda row, arm=arm: study.ratio(row[0], "serial", arm, study.competitor_of(row), 1),
                graded_of=serial["graded_of"],
            )
            counted = max(counted, count)
            left_out = max(left_out, descriptive)
            cells.append(as_ratio(med))
        cells[1] = str(counted)
        cells[2] = str(left_out)
        rows.append("| " + " | ".join(cells) + " |")
    table(out, header, [":--", "--:", "--:"] + ["--:"] * len(arms_only), rows)

    out.append("## Medians by grading stratum")
    out.append("")
    out.append("The four strata the decision rule names. `dense-selected` and")
    out.append("`sparse-selected` come from the routing rule, `maxdim-1` and")
    out.append("`maxdim-2` from the entry's dimension. An entry above dimension 2")
    out.append("enters a routing stratum only. Each median covers the graded entries")
    out.append("of its stratum, and the descriptive column counts the entries it left")
    out.append("out.")
    out.append("")
    rows = []
    for name in GRADING_STRATA:
        cells = [name, "", ""]
        counted, left_out = 0, 0
        for arm in arms_only:
            med, count, descriptive = stratum_median(
                study,
                study.ok_entries(),
                lambda row, arm=arm: study.ratio(row[0], "serial", arm, study.competitor_of(row), 1),
                name,
                graded_of=serial["graded_of"],
            )
            counted = max(counted, count)
            left_out = max(left_out, descriptive)
            cells.append(as_ratio(med))
        cells[1] = str(counted)
        cells[2] = str(left_out)
        rows.append("| " + " | ".join(cells) + " |")
    table(out, ["grading stratum", "graded entries", "descriptive"] + arms_only,
          [":--", "--:", "--:"] + ["--:"] * len(arms_only), rows)

    out.append("## Peak RSS")
    out.append("")
    out.append("Megabytes, the largest VmHWM of the timed serial runs. giotto-ph runs")
    out.append("in process and reports none. No grade reads this table.")
    out.append("")
    table(
        out,
        ["id"] + columns,
        [":--"] + ["--:"] * len(columns),
        [
            "| " + " | ".join(
                [row[0]] + [megabytes(study.rss(row[0], "serial", arm, 1)) for arm in columns]
            ) + " |"
            for row in study.entries
        ],
    )

    out.append(f"## Multicore, {cores} physical cores")
    out.append("")
    out.append(f"H3 at {cores} physical cores against giotto-ph at {cores}, on the entries")
    out.append("the corpus marks `multicore = true`. holos pays its process")
    out.append("start and its input parse inside the number; the giotto-ph clock wraps")
    out.append("`ripser_parallel` alone. The comparison favors giotto-ph.")
    out.append("")
    rows = []
    for row in study.entries:
        holos = study.wall(row[0], "multicore", "h3", cores)
        if holos is None:
            continue
        gph = study.wall(row[0], "multicore", "gph", cores)
        rows.append("| " + " | ".join([
            row[0], row[2], row[3], seconds(holos), seconds(gph),
            as_ratio(study.ratio(row[0], "multicore", "h3", "gph", cores)),
            as_ratio(study.ratio(row[0], "multicore", "h3aa", "h3", cores)),
            standing(study, row, multicore),
        ]) + " |")
    table(out, ["id", "engine", "max dim", "h3 wall", "giotto-ph wall", "h3 over gph",
                "A/A", "graded"],
          [":--", ":--", "--:", "--:", "--:", "--:", "--:", ":--"], rows)

    out.append("## CPU scaling")
    out.append("")
    out.append(f"Wall time in seconds at 1, 2, and {cores} physical cores without SMT,")
    out.append("the self-speedup of each product against its own 1-core time, and holos")
    out.append(f"over giotto-ph at each point. H2 runs at {cores} cores as well, as the")
    out.append("attribution diagnostic. This table carries no grade.")
    out.append("")
    points = sorted({
        int(threads)
        for (_, pass_name, arm, threads) in study.value
        if pass_name == "scaling" and arm in ("h3", "gph")
    }) or [1, 2, cores]
    header = ["id", "engine", "max dim", "n", "cores", "h3 wall", "giotto-ph wall",
              "h3 speedup", "giotto-ph speedup", "h3 over giotto-ph"]
    rows = []
    for row in study.scaling:
        base_holos = study.wall(row[0], "scaling", "h3", 1)
        base_gph = study.wall(row[0], "scaling", "gph", 1)
        for t in points:
            holos = study.wall(row[0], "scaling", "h3", t)
            gph = study.wall(row[0], "scaling", "gph", t)
            rows.append("| " + " | ".join([
                row[0], row[1], row[2], row[3], str(t), seconds(holos), seconds(gph),
                as_ratio(None if not (base_holos and holos) else base_holos / holos),
                as_ratio(None if not (base_gph and gph) else base_gph / gph),
                as_ratio(study.ratio(row[0], "scaling", "h3", "gph", t)),
            ]) + " |")
    table(out, header, [":--", ":--", "--:", "--:", "--:"] + ["--:"] * 5, rows)

    out.append("### Attribution")
    out.append("")
    out.append(f"H2 and H3, both at {cores} physical cores, on the same entries.")
    out.append("The ratio is H3 over H2 at that core count.")
    out.append("")
    rows = []
    for row in study.scaling:
        h2 = study.wall(row[0], "scaling", "h2", cores)
        h3 = study.wall(row[0], "scaling", "h3", cores)
        rows.append("| " + " | ".join([
            row[0], seconds(h2), seconds(h3),
            as_ratio(None if not (h2 and h3) else h3 / h2),
        ]) + " |")
    table(out, ["id", f"h2 at {cores}c", f"h3 at {cores}c", "h3 over h2"],
          [":--", "--:", "--:", "--:"], rows)

    if f64:
        out.append("## Matched-precision diagnostic")
        out.append("")
        out.append("holos stores f64 and stock ripser stores f32. These columns are the")
        out.append("audited double build of the same ripser source, at the same flags.")
        out.append("They are a diagnostic: they are never pooled with the stock arm, and")
        out.append("no grade reads them.")
        out.append("")
        header = ["id"] + [f"h3 over {name}" for name in f64] + [f"{name} over stock" for name in f64]
        rows = []
        for row in study.entries:
            cells = [row[0]]
            for name in f64:
                cells.append(as_ratio(study.ratio(row[0], "serial", "h3", name, 1)))
            for name in f64:
                stock_name = name[: -len("-f64")]
                cells.append(as_ratio(study.ratio(row[0], "serial", name, stock_name, 1)))
            rows.append("| " + " | ".join(cells) + " |")
        table(out, header, [":--"] + ["--:"] * (len(header) - 1), rows)

    out.append("## Noise band")
    out.append("")
    percentile = f"{study.options['band_percentile']:g}"
    slack = f"{1.0 + study.options['slack']:.2f}"
    out.append("The A/A control times a byte-identical copy of H3 as its own arm. Its")
    out.append("per-entry ratio against H3 is what the harness reports for two identical")
    out.append(f"binaries. The band of a pass is the {percentile}th percentile of the")
    out.append("absolute distance from 1.0 of those ratios, and a ratio inside the band")
    out.append(f"is a tie. The per-entry limit is the wider of {slack} and 1 + band.")
    out.append("")
    out.append("The band reads the graded entries only. An A/A ratio divides by the")
    out.append("`h3` median, so an entry whose `h3` median is under the short-run floor")
    out.append("is descriptive here and enters no band.")
    out.append("")
    rows = []
    for grade in (serial, multicore):
        rows.append("| " + " | ".join([
            grade["pass"], str(grade["threads"]), as_ratio(grade["band"]),
            str(grade["band_entries"]), as_ratio(grade["limit"]),
            str(grade["entries"]), str(grade["graded"]),
            str(len(grade["descriptive"])),
            f"{study.options['short_run']:.3f}",
        ]) + " |")
    table(out, ["pass", "threads", "band", "band entries", "per-entry limit",
                "entries", "graded", "descriptive", "short-run floor (s)"],
          [":--", "--:", "--:", "--:", "--:", "--:", "--:", "--:", "--:"], rows)

    out.append("## Grades")
    out.append("")
    out.append("Each clause of the frozen decision rule, with the numbers it read. The")
    out.append("strata come first, and the overall median is never read alone. Every")
    out.append("median names the number of graded entries it covers, and every")
    out.append("descriptive entry it left out. An entry is descriptive in a clause when")
    out.append("the median that clause divides by is under the short-run floor of")
    out.append(f"{study.options['short_run']:.3f} s.")
    out.append("")
    for grade, title, with_regression, competitor in (
        (serial, "Serial, one physical core, against stock ripser", True, "ripser"),
        (multicore, f"Multicore, {cores} physical cores, against giotto-ph", False, "giotto-ph"),
    ):
        out.append(f"### {title}")
        out.append("")
        rows = []
        for name, passed, detail in clauses(grade, with_regression, competitor):
            rows.append("| " + " | ".join([name, verdict(passed), detail]) + " |")
        table(out, ["clause", "verdict", "numbers"], [":--", ":--", ":--"], rows)
    out.append("The CPU scaling stratum is descriptive and carries no clause.")
    out.append("")
    return out


def text(study, serial, multicore):
    lines = []
    cores = study.options["physical_cores"]
    arms_only = [label for label, _, _ in study.arms if label != "h3aa"]
    for name in study.input_strata():
        subset = [row for row in study.ok_entries() if row[1] == name]
        for arm in arms_only:
            med, count, descriptive = stratum_median(
                study,
                subset,
                lambda row, arm=arm: study.ratio(row[0], "serial", arm, study.competitor_of(row), 1),
                graded_of=serial["graded_of"],
            )
            lines.append(
                f"INPUT_STRATUM stratum={name} arm={arm} median_over_ripser="
                f"{as_ratio(med)} graded_entries={count} descriptive={descriptive}"
            )
    for grade in (serial, multicore):
        for name in GRADING_STRATA:
            med, count, descriptive = grade["strata"].get(name, (None, 0, 0))
            lines.append(
                f"GRADING_STRATUM pass={grade['pass']} threads={grade['threads']} "
                f"stratum={name} median={as_ratio(med)} graded_entries={count} "
                f"descriptive={descriptive}"
            )
        med, count, descriptive = grade["overall"]
        lines.append(
            f"OVERALL pass={grade['pass']} threads={grade['threads']} headline_median="
            f"{as_ratio(med)} graded_entries={count} descriptive={descriptive}"
        )
        lines.append(
            f"NOISE pass={grade['pass']} band={as_ratio(grade['band'])} "
            f"band_entries={grade['band_entries']} "
            f"entry_limit={as_ratio(grade['limit'])} entries={grade['entries']} "
            f"graded={grade['graded']} descriptive={len(grade['descriptive'])}"
        )
        for name, value in grade["violations"]:
            lines.append(f"VIOLATION pass={grade['pass']} entry={name} ratio={as_ratio(value)}")
    for name, passed, detail in clauses(serial, True, "ripser"):
        lines.append(f"GRADE pass=serial clause=\"{name}\" verdict={verdict(passed)} {detail}")
    for name, passed, detail in clauses(multicore, False, "giotto-ph"):
        lines.append(f"GRADE pass=multicore clause=\"{name}\" verdict={verdict(passed)} {detail}")
    for row in study.scaling:
        for t in (1, 2, cores):
            lines.append(
                f"SCALING entry={row[0]} threads={t} "
                f"h3_s={seconds(study.wall(row[0], 'scaling', 'h3', t))} "
                f"gph_s={seconds(study.wall(row[0], 'scaling', 'gph', t))} "
                f"h3_over_gph={as_ratio(study.ratio(row[0], 'scaling', 'h3', 'gph', t))}"
            )
    return lines


def main():
    argv = sys.argv[1:]
    as_text = False
    options = {
        "short_run": 0.02,
        "slack": 0.05,
        "band_percentile": 95.0,
        "physical_cores": 4,
    }
    positional = []
    while argv:
        item = argv.pop(0)
        if item == "--text":
            as_text = True
        elif item == "--short-run":
            options["short_run"] = float(argv.pop(0))
        elif item == "--slack":
            options["slack"] = float(argv.pop(0))
        elif item == "--band-percentile":
            options["band_percentile"] = float(argv.pop(0))
        elif item == "--physical-cores":
            options["physical_cores"] = int(argv.pop(0))
        else:
            positional.append(item)
    if len(positional) != 4:
        sys.exit(
            "usage: north_star_tables.py [--text] [options] "
            "ENTRY_META TOTALS SCALING_META ARMS"
        )
    meta_path, totals_path, scaling_path, arms_text = positional

    arms = []
    for item in arms_text.split():
        label, commit, sha = item.split(":")
        arms.append((label, commit, sha))
    study = Study(
        read_rows(meta_path, 9),
        read_rows(totals_path, 7),
        read_rows(scaling_path, 6),
        arms,
        options,
    )
    cores = options["physical_cores"]
    serial = grade_pass(study, "serial", 1, study.competitor_of)
    multicore = grade_pass(study, "multicore", cores, lambda row: "gph")
    if as_text:
        print("\n".join(text(study, serial, multicore)))
    else:
        print("\n".join(markdown(study, serial, multicore)).rstrip() + "\n")


if __name__ == "__main__":
    main()
