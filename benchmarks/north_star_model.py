"""Core records and grading calculations for the north-star study."""

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
    neighboring order statistics, the rule benchmarks/_common.sh uses."""
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
                number(med),
                number(iqr),
                number(rss),
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


def stratum_median(
    study, rows, ratio_of, name=None, headline_only=False, graded_of=None
):
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


class PassRatios:
    """The ratio functions used to grade one benchmark pass."""

    def __init__(self, study, pass_name, threads, competitor_of, arm):
        self.study = study
        self.pass_name = pass_name
        self.threads = threads
        self.competitor_of = competitor_of
        self.arm = arm

    def graded(self, row):
        return self.study.graded(
            row,
            self.pass_name,
            self.competitor_of(row),
            self.threads,
        )

    def ratio(self, row):
        return self.study.ratio(
            row[0],
            self.pass_name,
            self.arm,
            self.competitor_of(row),
            self.threads,
        )

    def regression_graded(self, row):
        return self.study.regression_graded(row, self.pass_name, self.threads)

    def regression_ratio(self, row):
        return self.study.regression_ratio(row[0], self.pass_name, self.threads)


def pass_strata(study, rows, ratio_of, graded_of):
    return {
        name: stratum_median(study, rows, ratio_of, name, graded_of=graded_of)
        for name in GRADING_STRATA
    }


def limit_violations(rows, ratio_of, graded_of, limit):
    violations = []
    for row in rows:
        if not graded_of(row):
            continue
        got = ratio_of(row)
        if got is not None and got > limit:
            violations.append((row[0], got))
    return violations


def regression_grade(study, rows, ratios, limit):
    if not study.earlier_arms:
        return {}, []
    strata = pass_strata(
        study,
        rows,
        ratios.regression_ratio,
        ratios.regression_graded,
    )
    violations = limit_violations(
        rows,
        ratios.regression_ratio,
        ratios.regression_graded,
        limit,
    )
    return strata, violations


def grade_pass(study, pass_name, threads, competitor_of, arm="h3"):
    """Return every figure that the decision rule reads for one pass."""
    rows = [
        row for row in study.ok_entries() if study.wall(row[0], pass_name, arm, threads)
    ]
    ratios = PassRatios(study, pass_name, threads, competitor_of, arm)
    band, band_entries = study.band(pass_name, threads, ratios.graded)
    limit = study.entry_limit(band)
    strata = pass_strata(study, rows, ratios.ratio, ratios.graded)
    overall = stratum_median(
        study,
        rows,
        ratios.ratio,
        None,
        headline_only=True,
        graded_of=ratios.graded,
    )
    violations = limit_violations(rows, ratios.ratio, ratios.graded, limit)
    regression_strata, regression_violations = regression_grade(
        study, rows, ratios, limit
    )
    descriptive = [row[0] for row in rows if not ratios.graded(row)]
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
        "graded_of": ratios.graded,
        "regression_graded_of": ratios.regression_graded,
    }
