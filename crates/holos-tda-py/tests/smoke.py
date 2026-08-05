"""Smoke tests for the built holos-tda wheel. Plain asserts, no test
framework. Run as `python py/tests/smoke.py` in an env with the wheel
installed."""

import math
import subprocess
import sys

import holos_tda

SQRT2 = math.sqrt(2.0)


def close(a, b, tol=1e-12):
    return a == b or abs(a - b) <= tol


# Unit square from points: 4 H0 bars (one essential), one H1 bar [1, sqrt 2).
bars = holos_tda.rips_points([[0, 0], [1, 0], [1, 1], [0, 1]], max_dim=1)
assert len([b for b in bars if b[0] == 0]) == 4
assert len([b for b in bars if b[0] == 0 and b[2] == math.inf]) == 1
(h1,) = [b for b in bars if b[0] == 1]
assert close(h1[1], 1.0) and close(h1[2], SQRT2)

# Condensed input follows SciPy pdist (upper-triangle) order. This n=4
# asymmetric matrix distinguishes pdist order from lower-triangle order:
# pdist [d01, d02, d03, d12, d13, d23] = [0.5, 0.5, 1, 10, 5, 6] gives H0
# finite deaths [0.5, 0.5, 1]. A lower-triangle misread gives [0.5, 0.5, 5].
bars = holos_tda.rips_condensed([0.5, 0.5, 1.0, 10.0, 5.0, 6.0], max_dim=0)
deaths = sorted(b[2] for b in bars if b[2] != math.inf)
assert deaths == [0.5, 0.5, 1.0], deaths

# pdist and points agree on the square.
pd = [1.0, SQRT2, 1.0, 1.0, SQRT2, 1.0]
assert holos_tda.rips_condensed(pd, max_dim=1) == holos_tda.rips_points(
    [[0, 0], [1, 0], [1, 1], [0, 1]], max_dim=1
)

# Sparse 4-cycle: the hole never fills (no diagonals listed).
bars = holos_tda.rips_sparse(4, [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)])
(h1,) = [b for b in bars if b[0] == 1]
assert close(h1[1], 1.0) and h1[2] == math.inf

# threads=2 yields the identical diagram through every entry point.
sq = [[0, 0], [1, 0], [1, 1], [0, 1]]
assert holos_tda.rips_points(sq, max_dim=1, threads=2) == holos_tda.rips_points(
    sq, max_dim=1
)
assert holos_tda.rips_condensed(pd, max_dim=1, threads=2) == holos_tda.rips_condensed(
    pd, max_dim=1
)
cyc = [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]
assert holos_tda.rips_sparse(4, cyc, threads=2) == holos_tda.rips_sparse(4, cyc)

# Coefficients: valid odd prime works, composite raises.
holos_tda.rips_points([[0, 0], [1, 0]], modulus=3)
try:
    holos_tda.rips_points([[0, 0], [1, 0]], modulus=4)
except ValueError as e:
    assert "prime" in str(e)
else:
    raise AssertionError("modulus=4 must raise ValueError")

# Console script is the real CLI.
out = subprocess.run(
    ["holos-tda", "--version"], capture_output=True, text=True, check=True
)
assert holos_tda.__version__ in out.stdout

print("smoke OK", holos_tda.__version__, holos_tda.GIT_HASH)
sys.exit(0)
