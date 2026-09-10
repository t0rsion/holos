import sys

from . import _core


def main(argv=None):
    """Entry point for the ``holos-tda`` console script."""
    args = list(sys.argv[1:]) if argv is None else list(argv)
    raise SystemExit(_core.run_cli(["holos"] + args))
