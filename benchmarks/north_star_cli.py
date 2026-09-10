"""Command-line parsing and study loading."""

import sys

if __package__:
    from .north_star_model import Study, read_rows
else:
    from north_star_model import Study, read_rows


def default_options():
    return {
        "short_run": 0.02,
        "slack": 0.05,
        "band_percentile": 95.0,
        "physical_cores": 4,
    }


def consume_option(item, argv, options):
    specifications = {
        "--short-run": ("short_run", float),
        "--slack": ("slack", float),
        "--band-percentile": ("band_percentile", float),
        "--physical-cores": ("physical_cores", int),
    }
    specification = specifications.get(item)
    if specification is None:
        return False
    name, convert = specification
    options[name] = convert(argv.pop(0))
    return True


def parse_cli(argv):
    as_text = False
    options = default_options()
    positional = []
    while argv:
        item = argv.pop(0)
        if item == "--text":
            as_text = True
        elif not consume_option(item, argv, options):
            positional.append(item)
    if len(positional) != 4:
        sys.exit(
            "usage: north_star_tables.py [--text] [options] "
            "ENTRY_META TOTALS SCALING_META ARMS"
        )
    return as_text, options, positional


def parse_arms(arms_text):
    arms = []
    for item in arms_text.split():
        label, commit, sha = item.split(":")
        arms.append((label, commit, sha))
    return arms


def load_study(paths, options):
    meta_path, totals_path, scaling_path, arms_text = paths
    return Study(
        read_rows(meta_path, 9),
        read_rows(totals_path, 7),
        read_rows(scaling_path, 6),
        parse_arms(arms_text),
        options,
    )
