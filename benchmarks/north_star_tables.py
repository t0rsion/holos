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

if __package__:
    from .north_star_cli import load_study, parse_cli
    from .north_star_model import grade_pass
    from .north_star_output import markdown, text
else:
    from north_star_cli import load_study, parse_cli
    from north_star_model import grade_pass
    from north_star_output import markdown, text


def main():
    as_text, options, paths = parse_cli(sys.argv[1:])
    study = load_study(paths, options)
    cores = options["physical_cores"]
    serial = grade_pass(study, "serial", 1, study.competitor_of)
    multicore = grade_pass(study, "multicore", cores, lambda row: "gph")
    if as_text:
        print("\n".join(text(study, serial, multicore)))
    else:
        print("\n".join(markdown(study, serial, multicore)).rstrip() + "\n")


if __name__ == "__main__":
    main()
