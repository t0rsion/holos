"""Markdown tables for entries, serial runs, and multicore runs."""

if __package__:
    from .north_star_grades import section, standing
    from .north_star_model import (
        GRADING_STRATA,
        as_ratio,
        megabytes,
        seconds,
        stratum_median,
        table,
    )
else:
    from north_star_grades import section, standing
    from north_star_model import (
        GRADING_STRATA,
        as_ratio,
        megabytes,
        seconds,
        stratum_median,
        table,
    )


def serial_competitors(study, serial_arms):
    return sorted(
        {
            arm
            for (_, pass_name, arm, _) in study.value
            if pass_name == "serial" and arm not in serial_arms
        }
    )


def precision_arms(competitors, matched):
    return [name for name in competitors if name.endswith("-f64") == matched]


def report_arms(study):
    serial_arms = [label for label, _, _ in study.arms]
    competitors = serial_competitors(study, serial_arms)
    stock = precision_arms(competitors, False)
    f64 = precision_arms(competitors, True)
    return serial_arms, stock, f64


def append_entries(out, study):
    section(
        out,
        "Entries",
        [
            "The engine column is the engine the frozen routing rule of H3 selects",
            "for the entry, computed from the point count and the edge count at the",
            "threshold. It is a property of the input and the frozen constants, not",
            "of a measurement. A void entry lost its agreement pass and enters no",
            "median below.",
        ],
    )
    table(
        out,
        [
            "id",
            "input stratum",
            "engine",
            "max dim",
            "n",
            "edges",
            "headline",
            "competitor",
            "status",
        ],
        [":--", ":--", ":--", "--:", "--:", "--:", ":--", ":--", ":--"],
        ["| " + " | ".join(row) + " |" for row in study.entries],
    )


def serial_total_rows(study, columns):
    rows = []
    for row in study.entries:
        cells = [row[0]]
        cells += [seconds(study.wall(row[0], "serial", arm, 1)) for arm in columns]
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_serial_totals(out, study, columns):
    section(
        out,
        "Serial fresh-process totals",
        [
            "Seconds on one physical core, median over the timed repetitions of one",
            "fresh process per run. `h3aa` is the A/A control: the same bytes as",
            "`h3`, timed as its own arm.",
        ],
    )
    table(
        out,
        ["id"] + columns,
        [":--"] + ["--:"] * len(columns),
        serial_total_rows(study, columns),
    )


def serial_ratio_rows(study, serial, arms):
    rows = []
    for row in study.entries:
        cells = [row[0], row[2], row[3]]
        for arm in arms:
            ratio = study.ratio(row[0], "serial", arm, study.competitor_of(row), 1)
            cells.append(as_ratio(ratio))
        cells.append(as_ratio(study.ratio(row[0], "serial", "h3aa", "h3", 1)))
        cells.append(standing(study, row, serial))
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_serial_ratios(out, study, serial, serial_arms):
    section(
        out,
        "Serial ratios against ripser",
        [
            "Each holos arm over the entry's own ripser build, on the same file at",
            "the same threshold. Below 1.000 means holos is faster. The A/A column",
            "is the control ratio against `h3`, not against ripser: it is what the",
            "harness reports for two identical binaries.",
            "",
            "The graded column is `descriptive` when the entry's ripser median is",
            f"under the short-run floor of {study.options['short_run']:.3f} s. Such a",
            "row is reported, and it enters no median, no per-entry clause, no",
            "regression aggregate, and no band.",
        ],
    )
    arms = [arm for arm in serial_arms if arm != "h3aa"]
    header = ["id", "engine", "max dim"] + arms + ["h3aa over h3", "graded"]
    aligns = [":--", ":--", "--:"] + ["--:"] * (len(header) - 4) + [":--"]
    table(out, header, aligns, serial_ratio_rows(study, serial, arms))


def arm_median(study, rows, serial, arm, stratum=None):
    return stratum_median(
        study,
        rows,
        lambda row: study.ratio(row[0], "serial", arm, study.competitor_of(row), 1),
        stratum,
        graded_of=serial["graded_of"],
    )


def input_median_rows(study, serial, arms):
    rows = []
    for name in study.input_strata():
        subset = [row for row in study.ok_entries() if row[1] == name]
        cells = [name, "", ""]
        counted, left_out = 0, 0
        for arm in arms:
            med, count, descriptive = arm_median(study, subset, serial, arm)
            counted = max(counted, count)
            left_out = max(left_out, descriptive)
            cells.append(as_ratio(med))
        cells[1], cells[2] = str(counted), str(left_out)
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_input_medians(out, study, serial, arms):
    section(
        out,
        "Medians by input stratum",
        [
            "Median serial ratio inside each input stratum, one column per holos",
            "arm, with the number of graded entries behind the median and the",
            "number of descriptive entries it left out. A median over unlike",
            "regimes hides the regime that lost.",
        ],
    )
    header = ["input stratum", "graded entries", "descriptive"] + arms
    aligns = [":--", "--:", "--:"] + ["--:"] * len(arms)
    table(out, header, aligns, input_median_rows(study, serial, arms))


def grading_median_rows(study, serial, arms):
    rows = []
    for name in GRADING_STRATA:
        cells = [name, "", ""]
        counted, left_out = 0, 0
        for arm in arms:
            med, count, descriptive = arm_median(
                study, study.ok_entries(), serial, arm, name
            )
            counted = max(counted, count)
            left_out = max(left_out, descriptive)
            cells.append(as_ratio(med))
        cells[1], cells[2] = str(counted), str(left_out)
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_grading_medians(out, study, serial, arms):
    section(
        out,
        "Medians by grading stratum",
        [
            "The four strata the decision rule names. `dense-selected` and",
            "`sparse-selected` come from the routing rule, `maxdim-1` and",
            "`maxdim-2` from the entry's dimension. An entry above dimension 2",
            "enters a routing stratum only. Each median covers the graded entries",
            "of its stratum, and the descriptive column counts the entries it left",
            "out.",
        ],
    )
    header = ["grading stratum", "graded entries", "descriptive"] + arms
    aligns = [":--", "--:", "--:"] + ["--:"] * len(arms)
    table(out, header, aligns, grading_median_rows(study, serial, arms))


def append_peak_rss(out, study, columns):
    section(
        out,
        "Peak RSS",
        [
            "Megabytes, the largest VmHWM of the timed serial runs. giotto-ph runs",
            "in process and reports none. No grade reads this table.",
        ],
    )
    rows = []
    for row in study.entries:
        cells = [row[0]]
        cells += [megabytes(study.rss(row[0], "serial", arm, 1)) for arm in columns]
        rows.append("| " + " | ".join(cells) + " |")
    table(out, ["id"] + columns, [":--"] + ["--:"] * len(columns), rows)


def multicore_rows(study, multicore, cores):
    rows = []
    for row in study.entries:
        holos = study.wall(row[0], "multicore", "h3", cores)
        if holos is None:
            continue
        gph = study.wall(row[0], "multicore", "gph", cores)
        cells = [
            row[0],
            row[2],
            row[3],
            seconds(holos),
            seconds(gph),
            as_ratio(study.ratio(row[0], "multicore", "h3", "gph", cores)),
            as_ratio(study.ratio(row[0], "multicore", "h3aa", "h3", cores)),
            standing(study, row, multicore),
        ]
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_multicore(out, study, multicore, cores):
    section(
        out,
        f"Multicore, {cores} physical cores",
        [
            f"H3 at {cores} physical cores against giotto-ph at {cores}, on the entries",
            "the corpus marks `multicore = true`. holos pays its process",
            "start and its input parse inside the number; the giotto-ph clock wraps",
            "`ripser_parallel` alone. The comparison favors giotto-ph.",
        ],
    )
    header = [
        "id",
        "engine",
        "max dim",
        "h3 wall",
        "giotto-ph wall",
        "h3 over gph",
        "A/A",
        "graded",
    ]
    aligns = [":--", ":--", "--:", "--:", "--:", "--:", "--:", ":--"]
    table(out, header, aligns, multicore_rows(study, multicore, cores))
