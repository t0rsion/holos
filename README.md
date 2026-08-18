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
suite. The Rust crate is [`holos-tda`](https://crates.io/crates/holos-tda)
(library path `holos_tda`, binary `holos`). The Python package is
[`holos-tda`](https://pypi.org/project/holos-tda/) (import `holos_tda`).

## Status

Work in progress. Dimensions 0 and 1 are the primary target, and higher
dimensions run through the same dimension-generic core. The engine runs
serial by default. `--threads` sets the worker budget for the parallel
reducer. With a parallel collapse schedule selected, `--threads` is the
budget for the edge collapse as well. The diagram is identical at any
thread count. See "Correctness" for what the release gates cover.

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
# Point cloud (CSV: one point per line, comma or whitespace separated),
# H0 and H1, threshold defaults to the enclosing radius:
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

holos infers the input format from the file extension. `.csv`, `.pts`,
and `.xyz` are point clouds; anything else is a lower-distance matrix.
Sparse input needs an explicit `--format`, which also overrides the
inference. The diagram goes to stdout, and computation metadata goes to
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
three accept `max_dim`, `threshold`, `modulus`, `threads`, and
`collapse_edges`. The `holos-tda` script is the same CLI as the Rust
binary.

## Edge collapse

Edge collapse is optional preprocessing. It removes edges whose absence
cannot change any bar, then the engine runs on the smaller graph. An edge
qualifies only when, at every scale from the edge's own value to the end
of the filtration, some common neighbor of its endpoints is joined to
every other common neighbor at that scale. Removing such an edge leaves
the persistent homology of the flag filtration unchanged in every
dimension. The tests require bar-for-bar equality with the uncollapsed
run, with no tolerance. They cover several coefficient fields,
thresholds, and thread counts, and every optimization-toggle
combination.

The collapse sweeps the edges in passes and deletes each qualifying edge
as soon as it is found. This serial schedule is the default and, in the
registered studies below, the fastest end to end on most inputs.

Two parallel schedules are available. The ordered schedule runs the same
sweep with worker threads. The workers test a window of upcoming edges
against one frozen state of the graph, speculatively. The sweep then
walks the window in its own order and reuses a test only when no
deletion committed since that test could have changed it. The ordered
schedule reproduces the serial result exactly, bit for bit, at any
worker count. The rounds schedule deletes a batch of provably
independent edges per round. Its result does not depend on the worker
count either, but it is not the serial result, and on some inputs it
keeps far fewer edges. Which edges survive can differ between the
schedules; the diagram never does.

The collapse records every removal. The standalone API returns a certificate
that lists each removed edge, its value, the pass (or, for the rounds
schedule, the round) it was removed in, and the witnesses that justify
it, together with the reduced graph.

An independent verifier replays the certificate. It rebuilds the graph
and checks each recorded witness directly at every scale where the
edge's neighborhood changes; between those scales the checks carry over
unchanged. It then confirms that the reduced graph has no removable edge
left. For a rounds certificate it also checks that the removals of each
round are independent of each other. The verifier shares no sweep,
scheduling, or witness-selection code with the collapse, so a
certificate that passes has been checked twice by different means. The
verifier certifies that every recorded removal preserves the barcode and
that the reduced graph has no removable edge left. It does not certify
that a run followed any particular scheduling policy. The certificate is
an in-process value: there is no file format for it yet.

```sh
holos points.csv --collapse-edges                              # serial schedule
holos points.csv --collapse-edges --collapse-schedule ordered --threads 8
```

```rust
let params = RipsParams::new(1).with_edge_collapse();
let parallel = RipsParams::new(1)
    .with_threads(8)
    .with_collapse_schedule(holos_tda::CollapseSchedule::Ordered);
```

```python
bars = holos_tda.rips_points(points, max_dim=1, collapse_edges=True)
```

For the certificate itself, use `collapse::collapse_dense` or
`collapse::collapse_sparse`, which take the threshold. The ordered
schedule is `collapse::collapse_dense_ordered_parallel` and
`collapse::collapse_sparse_ordered_parallel`, and the rounds schedule is
`collapse::collapse_dense_rounds_parallel` and
`collapse::collapse_sparse_rounds_parallel`; these take a worker count as
well. The rounds schedule writes algorithm version 2 certificates, which
the verifier checks round by round. All of them return the reduced
matrix, the certificate, and run counters. The reduced graph does not
depend on the coefficient field, the homology dimension, or the thread
count, so one collapse can serve many runs.

Collapse is off by default. Finding the removable edges costs time, and
the collapse saves time only when it finds enough of them, so whether it
pays depends on the input.

<!-- Break-even numbers from benchmarks/results_collapse_confirm.md (v0.4.0 release records); scaling numbers from benchmarks/results_ordered_confirm.md and benchmarks/results_rounds_confirm.md. -->

In the registered break-even study (held-out confirmation set, serial
reducer, maxdim 2), the serial collapse won end to end on the cube family
at every threshold fraction (median 2.85x, range 1.55x to 3.55x) and on
the clusters family (median 2.22x). The sphere family's median was 3.80x
over a wide range (0.62x to 6.99x: the near-full-radius entry loses). The
torus family was within noise of break-even (median 1.14x). The mode that
isolates the collapse itself from the sparse enumerator confirmed the
gains come from the collapse. With the reducer already on eight threads,
or at maxdim 1, the collapse often costs more than it saves. The parallel
schedules do not change that picture.

In the registered scaling studies (held-out confirmation sets, four
physical cores with two threads each) the ordered schedule at four
workers ran the median headline entry at 0.84x the serial collapse speed
and the whole pipeline at 0.85x, and won end to end on three entries of
thirteen, by at most 1.13x. The rounds
schedule scaled its own collapse 4.8x from one worker to eight, but the
whole pipeline was slower than the serial one on every confirmed entry:
it tests many more edges to reach its fixed point, and that gap grows
with the edge count. Records for every number are attached to the
release. To measure your own data, run `benchmarks/collapse_bench.sh` in
the repository's `benchmarks/` directory. It reports edge counts, wall
time, and peak memory, and it validates the diagrams before it reports
any timing.

## Correctness

Tests compare every diagram against an independent oracle
(`src/oracle.rs`). The oracle is a textbook boundary-matrix reduction
over Z/p. It shares no code with the solver, down to a different inverse
algorithm. The comparison runs on exhaustive small spaces and on
randomized inputs. The tests compare larger inputs against ripser
(`RIPSER_BIN=... cargo test --test ripser_differential`); CI pins a fixed
ripser commit and also builds its coefficient-enabled variant for
`--modulus` runs. They check sparse input against the dense engine on the
same matrix and against ripser's sparse format. A projective-plane
fixture pins the torsion behavior: its H1 and H2 exist over Z/2 and
vanish over Z/3. Property tests cover permutation invariance, scaling
equivariance, and the optimization toggles (clearing, emergent pairs,
apparent pairs), which must not change the diagram.

The oracle and ripser gates cover H0, H1, and H2, over Z/2 and odd
primes. Higher dimensions run through the same generic code but are not
part of that gated claim. The parallel reducer must reproduce the serial
diagram exactly. A determinism gate recomputes random clouds, tie-heavy
grids, and degenerate fixtures at 1, 2, 4, and 8 threads over several
moduli, and requires bar-for-bar equality.

## Benchmarks

The timings below are single-threaded, against ripser on identical
lower-distance inputs (uniform random clouds in R^3). The harness fails
if the two tools' diagrams disagree, so every timing comes from a run
with matching barcodes.

<!-- Table summarized from benchmarks/results.md; regenerate with run.sh. -->

| points | threshold | maxdim | holos | ripser |
|-------:|:----------|-------:|------:|-------:|
| 500 | enclosing radius | 1 | 0.06 s | 0.05 s |
| 1000 | enclosing radius | 1 | 0.25 s | 0.21 s |
| 2000 | enclosing radius | 1 | 1.23 s | 0.95 s |
| 500 | 0.4 | 2 | 0.20 s | 0.15 s |

Peak memory is at parity with ripser across the run, including
maxdim 2.

The next table shows parallel scaling of the reducer on one cloud
(N=400, maxdim 2). The harness asserts that the diagram is identical at
every thread count.

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
