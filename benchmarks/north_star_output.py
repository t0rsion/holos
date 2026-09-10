"""Markdown and text output assembly."""

if __package__:
    from .north_star_diagnostics import (
        append_attribution,
        append_grades,
        append_noise,
        append_precision,
        append_scaling,
    )
    from .north_star_grades import clauses, verdict
    from .north_star_model import GRADING_STRATA, as_ratio, seconds
    from .north_star_report import (
        append_entries,
        append_grading_medians,
        append_input_medians,
        append_multicore,
        append_peak_rss,
        append_serial_ratios,
        append_serial_totals,
        arm_median,
        report_arms,
    )
else:
    from north_star_diagnostics import (
        append_attribution,
        append_grades,
        append_noise,
        append_precision,
        append_scaling,
    )
    from north_star_grades import clauses, verdict
    from north_star_model import GRADING_STRATA, as_ratio, seconds
    from north_star_report import (
        append_entries,
        append_grading_medians,
        append_input_medians,
        append_multicore,
        append_peak_rss,
        append_serial_ratios,
        append_serial_totals,
        arm_median,
        report_arms,
    )


def markdown(study, serial, multicore):
    out = []
    cores = study.options["physical_cores"]
    serial_arms, stock, f64 = report_arms(study)
    columns = serial_arms + stock + f64
    append_entries(out, study)
    append_serial_totals(out, study, columns)

    arms_only = [a for a in serial_arms if a != "h3aa"]
    append_serial_ratios(out, study, serial, serial_arms)
    append_input_medians(out, study, serial, arms_only)
    append_grading_medians(out, study, serial, arms_only)
    append_peak_rss(out, study, columns)
    append_multicore(out, study, multicore, cores)

    append_scaling(out, study, cores)
    append_attribution(out, study, cores)
    append_precision(out, study, f64)
    append_noise(out, study, serial, multicore)
    append_grades(out, study, serial, multicore, cores)
    return out


def input_stratum_text(study, serial, name, arms):
    lines = []
    subset = [row for row in study.ok_entries() if row[1] == name]
    for arm in arms:
        med, count, descriptive = arm_median(study, subset, serial, arm)
        lines.append(
            f"INPUT_STRATUM stratum={name} arm={arm} median_over_ripser="
            f"{as_ratio(med)} graded_entries={count} descriptive={descriptive}"
        )
    return lines


def input_text(study, serial):
    lines = []
    arms = [label for label, _, _ in study.arms if label != "h3aa"]
    for name in study.input_strata():
        lines.extend(input_stratum_text(study, serial, name, arms))
    return lines


def grade_text(grade):
    lines = []
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
        lines.append(
            f"VIOLATION pass={grade['pass']} entry={name} ratio={as_ratio(value)}"
        )
    return lines


def clause_text(grade, pass_name, with_regression, competitor):
    return [
        f'GRADE pass={pass_name} clause="{name}" verdict={verdict(passed)} {detail}'
        for name, passed, detail in clauses(grade, with_regression, competitor)
    ]


def scaling_text(study, cores):
    lines = []
    for row in study.scaling:
        for threads in (1, 2, cores):
            lines.append(
                f"SCALING entry={row[0]} threads={threads} "
                f"h3_s={seconds(study.wall(row[0], 'scaling', 'h3', threads))} "
                f"gph_s={seconds(study.wall(row[0], 'scaling', 'gph', threads))} "
                f"h3_over_gph={as_ratio(study.ratio(row[0], 'scaling', 'h3', 'gph', threads))}"
            )
    return lines


def text(study, serial, multicore):
    lines = input_text(study, serial)
    lines += grade_text(serial)
    lines += grade_text(multicore)
    lines += clause_text(serial, "serial", True, "ripser")
    lines += clause_text(multicore, "multicore", False, "giotto-ph")
    lines += scaling_text(study, study.options["physical_cores"])
    return lines
