# holos

Vietoris-Rips persistent homology in Rust: exact barcodes over Z/2 for point
clouds and precomputed distance matrices, computed by a ripser-class implicit
engine and checked against both an independent oracle and
[ripser](https://github.com/Ripser/ripser) itself.
Distributed as the crate [`holos-tda`](https://crates.io/crates/holos-tda)
(library path `holos_tda`, binary `holos`), usable as a library and as a CLI.

## Status

Early release, work in progress. v0.1 is Rust-only, serial, and aimed at
dimensions 0 and 1. Higher dimensions run through the same dimension-generic
core; see "Correctness" for what is actually certified.

## Install

```sh
cargo install holos-tda          # CLI (binary is named `holos`)
cargo add holos-tda              # library dependency
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

# Build identity (version, git commit, profile):
holos --version
```

Input format is inferred from the extension (`.csv`/`.pts`/`.xyz` are point
clouds, anything else lower-distance); `--format` overrides. The diagram goes
to stdout, computation metadata to stderr.

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

## Correctness

Tests compare every diagram against an independent oracle (`src/oracle.rs`,
a textbook boundary-matrix reduction that shares no code with the solver) on
exhaustive small spaces and randomized inputs, and against ripser on larger
ones (`RIPSER_BIN=... cargo test --test ripser_differential`; CI pins a
fixed ripser commit). Property tests cover permutation invariance, scaling
equivariance, and that the optimization toggles (clearing, emergent and
apparent pairs) change nothing.

The oracle and ripser tests certify H0, H1, and H2. Higher dimensions
compile and run through the same generic code, but they are not part of the
validated claim.

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
