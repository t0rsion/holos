# Maintained persistence comparison

This study compares Holos with the maintained vineyard implementation in
Dionysus 2.2.3. The pinned Python package exposes `Vineyard::transpose`, which
updates a reduced boundary matrix after one adjacent filtration transposition.
The package is distributed under the modified BSD license.

## Preregistered comparison

The input is the registered `HOLOSTEM1` temporal control at
`benchmarks/data/v10_public_program/snap-email-eu-dept3-z2.holostem`. The public
runner creates this ignored file from the SHA-256-pinned SNAP source. It contains
one fixed graph, 89 vertices, 973 listed edges, 4,289 induced triangles, and 104
snapshots. Each snapshot changes edge weights but keeps the graph cells fixed.
The comparison uses H0 and H1 over Z/2.

The Dionysus warm arm builds one vineyard at the first snapshot. It reaches each
later endpoint with adjacent transpositions. The update clock uses precomputed
endpoint orders. The full-update clock also constructs and sorts each endpoint
filtration. The fresh arm reduces every later endpoint independently. Every arm
records at least five timed repetitions. Untimed exactness computations run
before the timing arms.

The Python binding stores simplex data as `f32`. The runner therefore replaces
all distinct source edge weights with their global integer ranks. The ranks fit
exactly in `f32` and preserve every endpoint order. Holos receives the same rank
values for the exact comparison. The record names this encoding and does not
present rank values as source-scale weights.

The comparison fails unless all 104 H0 and H1 barcodes agree exactly. The record
also reports adjacent swaps and pairing switches. It does not measure Holos's
result-sensitive guard policy, changing graph topology, cocycles, or source-scale
vine geometry. Python-to-C++ call overhead remains part of the Dionysus timing.

## Reproduce

Download the registered SNAP archive, then run
`python3 benchmarks/v10_public_program_bench.py SOURCE` to verify the archive
and prepare the registered trajectory. Build the Holos release binary and
install the Dionysus requirement from `requirements.txt`. Then run:

```sh
python3 benchmarks/v10_external/run.py \
  --python /path/to/python \
  --holos target/release/holos
```

The runner writes ignored `results_v10_external_dionysus.txt` and
`results_v10_external_dionysus.md` files. It refuses a dirty worktree unless
`--allow-dirty` is given. A dirty smoke run is diagnostic only.

The external source is [Dionysus](https://github.com/mrzv/dionysus), with its
package requirement pinned in `requirements.txt`. BATS remains an optional historical
comparison under the same directory. Its update project is not the maintained
baseline for this study.
