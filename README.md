# holos

[![CI](https://github.com/t0rsion/holos/actions/workflows/ci.yml/badge.svg)](https://github.com/t0rsion/holos/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/holos-tda)](https://crates.io/crates/holos-tda)
[![docs.rs](https://img.shields.io/docsrs/holos-tda)](https://docs.rs/holos-tda)
![MSRV](https://img.shields.io/crates/msrv/holos-tda)

holos computes Vietoris-Rips persistent homology. It produces exact
barcodes over a prime field Z/p, with Z/2 as the default. It reads point
clouds and dense or sparse distance matrices. The engine is implicit, in
the same class as [ripser](https://github.com/Ripser/ripser). An
independent oracle and ripser itself check every diagram in the test
suite. The Rust crate
is [`holos-tda`](https://crates.io/crates/holos-tda) (library path
`holos_tda`, binary `holos`); the Python package is
[`holos-tda`](https://pypi.org/project/holos-tda/) (import `holos_tda`).

## Status

Work in progress. Dimensions 0 and 1 are the primary target. Higher
dimensions run through the same dimension-generic core. The engine runs
serial by default; `--threads` enables parallel reduction. The diagram is
identical at any thread count. See "Correctness" for the certified claims.

## Install

```sh
cargo install holos-tda          # CLI (binary is named `holos`)
cargo add holos-tda              # Rust library
pip install holos-tda            # Python library + `holos-tda` CLI
uvx holos-tda points.csv         # run the CLI without installing
```

or from a checkout: `cargo install --path .`

## CLI

```sh
# Point cloud (CSV: one point per line, comma/whitespace separated),
# H0 and H1, threshold defaulting to the enclosing radius:
holos points.csv

# Lower-distance matrix (ripser-compatible condensed lower triangle),
# explicit threshold, CSV output:
holos data.lower --format lower-distance --threshold 0.5 --output csv

# Sparse "i j d" triplets (unlisted pairs never enter the filtration),
# coefficients in Z/3:
holos graph.spr --format sparse --modulus 3

# Parallel reduction with 8 worker threads (same diagram as serial):
holos points.csv --threads 8

# Build identity (version, git commit, profile):
holos --version
```

holos infers the input format from the file extension: `.csv`, `.pts`,
and `.xyz` are point clouds; anything else is a lower-distance matrix.
Sparse input must be requested with `--format`, which also overrides the
inference. The diagram goes to stdout; computation metadata goes to
stderr.

## Library

```rust
use holos_tda::{DistanceMatrix, RipsParams};

fn main() -> holos_tda::Result<()> {
    let points = vec![vec![0.0, 0.0], vec![1.0, 0.0], vec![1.0, 1.0], vec![0.0, 1.0]];
    let dist = DistanceMatrix::from_points(&points)?;
    let diagram = holos_tda::rips_persistence(&dist, &RipsParams::new(1))?;
    for bar in &diagram.bars {
        println!("dim {}: [{}, {})", bar.dim, bar.birth, bar.death);
    }
    Ok(())
}
```

`RipsParams::with_modulus(p)` switches the coefficient field.
`RipsParams::threads` sets the number of reduction workers; 1 means
serial. For sparse input, use `SparseDistanceMatrix::from_triplets` with
`rips_persistence_sparse`.

## Python

```python
import holos_tda

bars = holos_tda.rips_points([[0, 0], [1, 0], [1, 1], [0, 1]], max_dim=1)
# [(0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, inf), (1, 1.0, 1.4142...)]
```

`rips_condensed` and `rips_sparse` mirror the Rust entry points. All
three accept `max_dim`, `threshold`, `modulus`, and `threads`. The
`holos-tda` script is the same CLI as the Rust binary.

## Correctness

Tests compare every diagram against an independent oracle
(`src/oracle.rs`). The oracle is a textbook boundary-matrix reduction
over Z/p. It shares no code with the solver, down to a different inverse
algorithm. The comparison runs on exhaustive small spaces and on
randomized inputs. Larger inputs are compared against ripser
(`RIPSER_BIN=... cargo test --test ripser_differential`); CI pins a fixed
ripser commit and also builds its coefficient-enabled variant for
`--modulus` runs. Sparse input is checked against the dense engine on the
same matrix and against ripser's sparse format. A projective-plane
fixture pins the torsion behavior: its H1 and H2 exist over Z/2 and
vanish over Z/3. Property tests cover permutation invariance, scaling
equivariance, and the optimization toggles (clearing, emergent pairs,
apparent pairs), which must not change the diagram.

The oracle and ripser tests certify H0, H1, and H2, over Z/2 and odd
primes. Higher dimensions run through the same generic code but are not
part of the certified claim. The parallel reducer must reproduce the
serial diagram exactly: a determinism gate recomputes random clouds,
tie-heavy grids, and degenerate fixtures at 1, 2, 4, and 8 threads over
several moduli and requires bar-for-bar equality.

## Benchmarks

Single-threaded, against ripser on identical lower-distance inputs
(uniform random clouds in R^3). The harness fails if the two tools'
diagrams disagree, so every timing below comes from a run with matching
barcodes.

<!-- Table summarized from benchmarks/results.md; regenerate with run.sh. -->

| points | threshold | maxdim | holos | ripser |
|-------:|:----------|-------:|------:|-------:|
| 500 | enclosing radius | 1 | 0.06 s | 0.05 s |
| 1000 | enclosing radius | 1 | 0.25 s | 0.21 s |
| 2000 | enclosing radius | 1 | 1.23 s | 0.95 s |
| 500 | 0.4 | 2 | 0.20 s | 0.15 s |

Peak memory is at parity with ripser across the run, including
maxdim 2.

Parallel scaling of the reducer on one cloud (N=400, maxdim 2). The
harness asserts that the diagram is identical at every thread count.

<!-- Table summarized from benchmarks/results_parallel.md; regenerate with parallel_scaling.sh. -->

| threads | wall | speedup |
|--:|--:|--:|
| 1 | 0.20 s | 1.0 |
| 4 | 0.09 s | 2.2 |
| 16 | 0.06 s | 3.3 |

`benchmarks/giotto_compare.sh` compares holos against giotto-ph's
`ripser_parallel` on identical clouds at matched thread counts. In our
runs the diagrams matched at every thread count, at wall-time parity.
`benchmarks/sparse_bench.sh` benchmarks sparse input against the dense
path.

Reproduce with the scripts in the repository's `benchmarks/` directory
(<https://github.com/t0rsion/holos>). Each script writes a full provenance
record (commit, binary hashes, build flags, CPU, allowed CPUs) beside its
table. The complete records behind these tables are attached to the
matching GitHub release.

## License

MIT or Apache-2.0, at your option.
