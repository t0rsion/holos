# holos

[![CI](https://github.com/t0rsion/holos/actions/workflows/ci.yml/badge.svg)](https://github.com/t0rsion/holos/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/holos-tda)](https://crates.io/crates/holos-tda)
[![docs.rs](https://img.shields.io/docsrs/holos-tda)](https://docs.rs/holos-tda)
![MSRV](https://img.shields.io/crates/msrv/holos-tda)

Vietoris-Rips persistent homology in Rust: exact barcodes over a prime
field Z/p (Z/2 by default) for point clouds and dense or sparse distance
matrices, computed by a ripser-class implicit engine and checked against
both an independent oracle and [ripser](https://github.com/Ripser/ripser)
itself. Distributed as the crate
[`holos-tda`](https://crates.io/crates/holos-tda) (library path `holos_tda`,
binary `holos`) and as the Python package
[`holos-tda`](https://pypi.org/project/holos-tda/) (import `holos_tda`).

## Status

Early release, work in progress. The engine is serial; dimensions 0 and 1
are the primary target, and higher dimensions run through the same
dimension-generic core. See "Correctness" for what is actually certified.

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

# Build identity (version, git commit, profile):
holos --version
```

Input format is inferred from the extension (`.csv`/`.pts`/`.xyz` are point
clouds, anything else lower-distance; sparse must be requested explicitly);
`--format` overrides. The diagram goes to stdout, computation metadata to
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

`RipsParams::with_modulus(p)` switches the coefficient field;
`SparseDistanceMatrix::from_triplets` plus `rips_persistence_sparse` handle
sparse input.

## Python

```python
import holos_tda

bars = holos_tda.rips_points([[0, 0], [1, 0], [1, 1], [0, 1]], max_dim=1)
# [(0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, inf), (1, 1.0, 1.4142...)]
```

`rips_condensed` and `rips_sparse` mirror the Rust entry points; all three
accept `max_dim`, `threshold`, and `modulus`. The `holos-tda` script is the
same CLI as the Rust binary.

## Correctness

Tests compare every diagram against an independent oracle (`src/oracle.rs`,
a textbook boundary-matrix reduction over Z/p that shares no code with the
solver, down to using a different inverse algorithm) on exhaustive small
spaces and randomized inputs, and against ripser on larger ones
(`RIPSER_BIN=... cargo test --test ripser_differential`; CI pins a fixed
ripser commit and also builds its coefficient-enabled variant for
`--modulus` runs). Sparse input is checked against the dense engine on the
same underlying matrix and against ripser's sparse format. A projective
plane fixture pins the torsion behavior: its H1 and H2 exist over Z/2 and
vanish over Z/3. Property tests cover permutation invariance, scaling
equivariance, and that the optimization toggles (clearing, emergent and
apparent pairs) change nothing.

The oracle and ripser tests certify H0, H1, and H2, over Z/2 and odd primes.
Higher dimensions compile and run through the same generic code, but they
are not part of the validated claim.

## Benchmarks

Single-threaded, against ripser on identical lower-distance inputs (uniform
random clouds in R^3). The harness fails if the two tools' diagrams ever
disagree, so every timing below comes from a run with matching barcodes.

<!-- Table summarized from benchmarks/results.md; regenerate with run.sh. -->

| points | threshold | maxdim | holos | ripser |
|-------:|:----------|-------:|------:|-------:|
| 500 | enclosing radius | 1 | 0.06 s | 0.06 s |
| 1000 | enclosing radius | 1 | 0.25 s | 0.22 s |
| 2000 | enclosing radius | 1 | 1.33 s | 0.97 s |
| 500 | 0.4 | 2 | 0.21 s | 0.15 s |

Peak memory is within about 15% of ripser at maxdim 1; the maxdim-2 run
currently uses about twice ripser's memory.

Reproduce with `benchmarks/run.sh`; it writes a full provenance record
(commit, binary hashes, build flags, CPU) to `benchmarks/results.md`. The
record behind the table above is attached to the matching GitHub release.

## License

MIT or Apache-2.0, at your option.
