"""Markdown tables for scaling and grading diagnostics."""

if __package__:
    from .north_star_grades import clauses, section, verdict
    from .north_star_model import as_ratio, seconds, table
else:
    from north_star_grades import clauses, section, verdict
    from north_star_model import as_ratio, seconds, table


def scaling_points(study, cores):
    points = {
        int(threads)
        for (_, pass_name, arm, threads) in study.value
        if pass_name == "scaling" and arm in ("h3", "gph")
    }
    return sorted(points) or [1, 2, cores]


def speedup(base, value):
    return None if not (base and value) else base / value


def scaling_entry_rows(study, row, points):
    rows = []
    base_holos = study.wall(row[0], "scaling", "h3", 1)
    base_gph = study.wall(row[0], "scaling", "gph", 1)
    for threads in points:
        holos = study.wall(row[0], "scaling", "h3", threads)
        gph = study.wall(row[0], "scaling", "gph", threads)
        cells = [
            row[0],
            row[1],
            row[2],
            row[3],
            str(threads),
            seconds(holos),
            seconds(gph),
            as_ratio(speedup(base_holos, holos)),
            as_ratio(speedup(base_gph, gph)),
            as_ratio(study.ratio(row[0], "scaling", "h3", "gph", threads)),
        ]
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def scaling_rows(study, points):
    rows = []
    for row in study.scaling:
        rows.extend(scaling_entry_rows(study, row, points))
    return rows


def append_scaling(out, study, cores):
    section(
        out,
        "CPU scaling",
        [
            f"Wall time in seconds at 1, 2, and {cores} physical cores without SMT,",
            "the self-speedup of each product against its own 1-core time, and holos",
            f"over giotto-ph at each point. H2 runs at {cores} cores as well, as the",
            "attribution diagnostic. This table carries no grade.",
        ],
    )
    header = [
        "id",
        "engine",
        "max dim",
        "n",
        "cores",
        "h3 wall",
        "giotto-ph wall",
        "h3 speedup",
        "giotto-ph speedup",
        "h3 over giotto-ph",
    ]
    aligns = [":--", ":--", "--:", "--:", "--:"] + ["--:"] * 5
    table(out, header, aligns, scaling_rows(study, scaling_points(study, cores)))


def append_attribution(out, study, cores):
    section(
        out,
        "Attribution",
        [
            f"H2 and H3, both at {cores} physical cores, on the same entries.",
            "The ratio is H3 over H2 at that core count.",
        ],
        level="###",
    )
    rows = []
    for row in study.scaling:
        h2 = study.wall(row[0], "scaling", "h2", cores)
        h3 = study.wall(row[0], "scaling", "h3", cores)
        ratio = None if not (h2 and h3) else h3 / h2
        cells = [row[0], seconds(h2), seconds(h3), as_ratio(ratio)]
        rows.append("| " + " | ".join(cells) + " |")
    table(
        out,
        ["id", f"h2 at {cores}c", f"h3 at {cores}c", "h3 over h2"],
        [":--", "--:", "--:", "--:"],
        rows,
    )


def precision_rows(study, f64):
    rows = []
    for row in study.entries:
        cells = [row[0]]
        for name in f64:
            cells.append(as_ratio(study.ratio(row[0], "serial", "h3", name, 1)))
        for name in f64:
            stock_name = name[: -len("-f64")]
            ratio = study.ratio(row[0], "serial", name, stock_name, 1)
            cells.append(as_ratio(ratio))
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_precision(out, study, f64):
    if not f64:
        return
    section(
        out,
        "Matched-precision diagnostic",
        [
            "holos stores f64 and stock ripser stores f32. These columns are the",
            "audited double build of the same ripser source, at the same flags.",
            "They are a diagnostic: they are never pooled with the stock arm, and",
            "no grade reads them.",
        ],
    )
    header = ["id"] + [f"h3 over {name}" for name in f64]
    header += [f"{name} over stock" for name in f64]
    table(
        out,
        header,
        [":--"] + ["--:"] * (len(header) - 1),
        precision_rows(study, f64),
    )


def noise_rows(study, serial, multicore):
    rows = []
    for grade in (serial, multicore):
        cells = [
            grade["pass"],
            str(grade["threads"]),
            as_ratio(grade["band"]),
            str(grade["band_entries"]),
            as_ratio(grade["limit"]),
            str(grade["entries"]),
            str(grade["graded"]),
            str(len(grade["descriptive"])),
            f"{study.options['short_run']:.3f}",
        ]
        rows.append("| " + " | ".join(cells) + " |")
    return rows


def append_noise(out, study, serial, multicore):
    percentile = f"{study.options['band_percentile']:g}"
    slack = f"{1.0 + study.options['slack']:.2f}"
    section(
        out,
        "Noise band",
        [
            "The A/A control times a byte-identical copy of H3 as its own arm. Its",
            "per-entry ratio against H3 is what the harness reports for two identical",
            f"binaries. The band of a pass is the {percentile}th percentile of the",
            "absolute distance from 1.0 of those ratios, and a ratio inside the band",
            f"is a tie. The per-entry limit is the wider of {slack} and 1 + band.",
            "",
            "The band reads the graded entries only. An A/A ratio divides by the",
            "`h3` median, so an entry whose `h3` median is under the short-run floor",
            "is descriptive here and enters no band.",
        ],
    )
    header = [
        "pass",
        "threads",
        "band",
        "band entries",
        "per-entry limit",
        "entries",
        "graded",
        "descriptive",
        "short-run floor (s)",
    ]
    aligns = [":--", "--:", "--:", "--:", "--:", "--:", "--:", "--:", "--:"]
    table(out, header, aligns, noise_rows(study, serial, multicore))


def append_grade_table(out, grade, title, with_regression, competitor):
    out.append(f"### {title}")
    out.append("")
    rows = []
    for name, passed, detail in clauses(grade, with_regression, competitor):
        rows.append("| " + " | ".join([name, verdict(passed), detail]) + " |")
    table(out, ["clause", "verdict", "numbers"], [":--", ":--", ":--"], rows)


def append_grades(out, study, serial, multicore, cores):
    section(
        out,
        "Grades",
        [
            "Each clause of the frozen decision rule, with the numbers it read. The",
            "strata come first, and the overall median is never read alone. Every",
            "median names the number of graded entries it covers, and every",
            "descriptive entry it left out. An entry is descriptive in a clause when",
            "the median that clause divides by is under the short-run floor of",
            f"{study.options['short_run']:.3f} s.",
        ],
    )
    append_grade_table(
        out,
        serial,
        "Serial, one physical core, against stock ripser",
        True,
        "ripser",
    )
    append_grade_table(
        out,
        multicore,
        f"Multicore, {cores} physical cores, against giotto-ph",
        False,
        "giotto-ph",
    )
    out.append("The CPU scaling stratum is descriptive and carries no clause.")
    out.append("")
