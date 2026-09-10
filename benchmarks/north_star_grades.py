"""Decision-rule clauses and report formatting helpers."""

if __package__:
    from .north_star_model import GRADING_STRATA, as_ratio
else:
    from north_star_model import GRADING_STRATA, as_ratio


def median_clause(name, competitor, med, count, descriptive):
    detail = f"median {as_ratio(med)} over {count} graded entries"
    if descriptive:
        detail += f", {descriptive} descriptive"
    return (
        f"stratum {name} at or below {competitor}",
        None if med is None else med <= 1.0,
        detail,
    )


def stratum_clauses(grade, competitor):
    clauses_out = [
        median_clause(name, competitor, *grade["strata"].get(name, (None, 0, 0)))
        for name in GRADING_STRATA
    ]
    med, count, descriptive = grade["overall"]
    _, passed, detail = median_clause("overall", competitor, med, count, descriptive)
    clauses_out.append(
        (
            f"overall median at or below {competitor}, headline entries",
            passed,
            detail,
        )
    )
    return clauses_out


def entry_limit_clause(grade):
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
    return "no entry above the per-entry limit", passed, detail


def worst_regression(grade):
    medians = [
        med for med, _, _ in grade["regression_strata"].values() if med is not None
    ]
    return max(medians, default=None)


def regression_detail(grade, worst):
    detail = (
        "worst stratum median "
        + as_ratio(worst)
        + "; "
        + ", ".join(
            f"{name} {as_ratio(med)} over {count} graded"
            for name, (med, count, _) in grade["regression_strata"].items()
        )
    )
    if grade["regression_violations"]:
        detail += "; entries over the limit: " + ", ".join(
            f"{name} {as_ratio(value)}"
            for name, value in grade["regression_violations"]
        )
    return detail


def regression_clause(grade):
    worst = worst_regression(grade)
    return (
        "no material regression against the best earlier arm",
        None
        if worst is None
        else (worst <= 1.05 and not grade["regression_violations"]),
        regression_detail(grade, worst),
    )


def clauses(grade, with_regression=True, competitor="ripser"):
    """Return the frozen clauses of one pass and the numbers behind each."""
    out = stratum_clauses(grade, competitor)
    out.append(entry_limit_clause(grade))
    if with_regression and grade["regression_strata"]:
        out.append(regression_clause(grade))
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


def section(out, title, paragraphs, level="##"):
    out.append(f"{level} {title}")
    out.append("")
    out.extend(paragraphs)
    out.append("")
