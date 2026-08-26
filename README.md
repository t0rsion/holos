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
dimensions run through the same dimension-generic core. A run is serial by
default. `--threads` is the worker budget for the whole run: the input
parse, the reduction, and, with a parallel collapse schedule selected, the
edge collapse. The diagram is identical at any thread count. See
"Correctness" for what the release gates cover.

## Install

```sh
cargo install holos-tda          # CLI (binary is named `holos`)
cargo add holos-tda              # Rust library
pip install holos-tda            # Python library + `holos-tda` CLI
uvx holos-tda points.csv         # run the CLI without installing
```

or from a checkout: `cargo install --path crates/holos-tda`

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

# Parallel parse and reduction with 8 worker threads (same diagram as
# serial):
holos points.csv --threads 8

# Force the engine for a dense input (auto is the default):
holos data.lower --format lower-distance --engine dense

# Forbid the full row-major matrix on a dense run (auto is the default):
holos data.lower --format lower-distance --dense-storage compact

# Build identity (version, git commit, profile):
holos --version
```

`--engine` picks the engine for a dense input, and `--dense-storage` picks
the form that engine reduces from. See "Engine" for both rules.

The diagram goes to stdout, and computation metadata goes to stderr.

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
`RipsParams::threads` is the worker budget; 1 means serial.
`RipsParams::with_engine` and `RipsParams::with_dense_storage` take the
same choices as `--engine` and `--dense-storage`. For sparse input, use
`SparseDistanceMatrix::from_triplets` with `rips_persistence_sparse`. To
read a file, use `holos_tda::io::read_point_cloud`,
`read_lower_distance_matrix`, or `read_sparse_matrix`; each takes the path
and a worker budget. To parse text a caller already holds, use
`io::parse_point_cloud`, `io::parse_condensed`, or `io::parse_triplets`.

## Python

```python
import holos_tda

bars = holos_tda.rips_points([[0, 0], [1, 0], [1, 1], [0, 1]], max_dim=1)
# [(0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, inf), (1, 1.0, 1.4142...)]
```

`rips_condensed` and `rips_sparse` mirror the Rust entry points. All
three accept `max_dim`, `threshold`, `modulus`, `threads`, and
`collapse_edges`. They select the engine and the storage form
automatically. The `holos-tda` script is the same CLI as the Rust binary.

## Input

holos infers the input format from the file extension. `.csv`, `.pts`, and
`.xyz` are point clouds; anything else is a lower-distance matrix. Sparse
input needs an explicit `--format`, which also overrides the inference.

Every reader accepts the same grammar: numbers separated by commas,
whitespace, or both, with blank lines and lines that start with `#` skipped.
A point cloud holds one point per line, and every point must have the same
dimension. A lower-distance matrix holds the condensed lower triangle, row
by row, in file order. A sparse file holds one `i j d` triplet per line, and
the point count is one more than the largest vertex index it sees; an
unlisted pair is absent and never enters the filtration.

A reader takes the file in windows of 16 MiB that end at a newline, so it
holds one window and the values parsed so far, not the whole text. With
`--threads` above 1, a second thread reads the next window while the current
one parses, and each window splits into one line chunk per worker at newline
boundaries. A text under one mebibyte parses serially at any thread count.
Each chunk knows the line number it starts at, so the values are the same
bit for bit and an error names the same line as a serial parse.

## Engine

holos has two engines, and both feed the same reduction. Neither stores a
boundary matrix: they enumerate cofacets on demand. The dense engine reads a
distance matrix and scans the vertex range for each cofacet. The sparse
engine reads a graph and merges neighbor lists. Sparse input always uses the
sparse engine. For dense input, routing picks one. The diagram is identical
under every engine and storage setting.

### Routing

`--engine auto`, the default, counts the edges of a dense input at the
resolved threshold and reduces a low-density input as that thresholded
graph. The rule is frozen and takes no argument. The input needs at least 32
points. The edge density must be at or below four fifths (`m / C(n, 2)`,
with `m` the finite pairs at or below the threshold). The conversion, at 24
bytes an edge and 24 a point, must fit a memory budget of 32 MiB, or the
bytes of the compact matrix, whichever is larger. An infinite threshold
routes as well. It admits every finite pair and no absent one, so a matrix
of mostly absent pairs still has a sparse graph. `--engine dense` keeps the
matrix. `--engine sparse` converts and ignores the budget, because it is an
explicit request. A sparse input is never routed, and neither is a collapse
run: the collapse already hands the sparse engine a graph.

### Dense storage forms

holos builds every `DistanceMatrix` compact, as the condensed lower triangle
of `n(n-1)/2` entries. `--dense-storage auto`, the default, converts it to a
full row-major matrix, which holds both triangles, when a frozen rule says
the run earns the added bytes. The rule reads the compact matrix size, a
budget on the bytes the second triangle adds, the edge count at the
threshold, and how many distances the cofacet diameter fold will read. In
the full form the fold reads one contiguous row per simplex vertex, where
the compact form reads a strided column. `--dense-storage compact` forbids
the conversion and `--dense-storage square` forces it. The choice comes
after routing, so a run the routing sends to the sparse engine builds no
full matrix. The conversion runs once and the caller keeps its own matrix,
so a run in the full form holds one and a half times that form. The peak
RSS of every arm and competitor is in the north-star record's Peak RSS
table.

### Sparse engine

The sparse engine's graph is one compressed block: one offset per vertex,
one array of neighbor indices, and one array of distances. The cofacet
enumerator merges the neighbor lists of a simplex's vertices in descending
vertex order and reports each cofacet as the merge finds it, so a search
that stops early stops the merge with it. The merge scans the index array
and reads a distance only where every list agrees. When the caller wants the
upper cofacets alone, the merge ends at the highest simplex vertex: the
candidates descend, and no candidate at or below that vertex is above every
simplex vertex.

### Adjacency rows

The dim-0 apparent test asks, for an edge `(u, v)` of diameter `d`, for the
largest vertex `w` joined to both endpoints at `d`. When the graph is dense
enough, the engine answers that from bitsets instead of neighbor lists. It
builds the adjacency rows, one bit per pair at or below the threshold, with
a rank index that reads any edge's distance in constant time. The dim-0 walk
then sets a second bit set, the activation rows, as it passes each edge. The
largest common bit of an edge's two activation rows names the youngest
cofacet of equal diameter, and the facet check reads at most two distances.

Memory decides whether the engine builds the rows. A cycle edge is an edge
beyond a spanning forest of the graph. The graph needs at least 1,024 cycle
edges, and the adjacency rows, their rank index, the activation rows, and
the distances together must cost at most 64 bytes an edge, which is about
what the graph itself costs. A sparser graph keeps the neighbor-list test.
`RipsParams::use_adjacency_rows` turns the rows off. The columns, their
order, and the diagram do not change either way.

### Parallel path

`--threads` sets the worker budget: the most workers a run may use, not the
number every step must use. The input parse, the edge sort, the dim-0
apparent test, the cofacet assembly, and the column reduction each turn
their own work estimate into a worker count under that budget. A step with
little work runs on one thread. The constants are frozen, and a decision
table in the tests pins them: the dim-0 apparent test takes a second worker
at 128 cycle edges, the assembly at 256 source simplices, and the sort at
20,000 elements. The diagram, the column order, and the pivot registry are
the same at every worker count.

## Edge collapse

Edge collapse is optional preprocessing. It removes edges whose absence
cannot change any bar of the flag filtration in any dimension. The engine
then runs on the smaller graph. An edge qualifies only when, at every scale
from the edge's own value to the end of the filtration, some common neighbor
of its endpoints is joined to every other common neighbor at that scale.
The tests require bar-for-bar equality with the uncollapsed run, with no
tolerance. They cover several coefficient fields, thresholds, and thread
counts, and every optimization-toggle combination.

The collapse sweeps the edges in passes and deletes each qualifying edge
as soon as it is found. This serial schedule is the default and, in the
registered studies below, the fastest end to end on most inputs.

Two parallel schedules are available. The ordered schedule runs the same
sweep with worker threads. The workers test a window of upcoming edges
against one frozen state of the graph, speculatively. The sweep then
walks the window in its own order and reuses a test only when no
deletion committed since that test could have changed it. The ordered
schedule reproduces the serial result bit for bit at any worker count.
The rounds schedule deletes a batch of provably independent edges per
round. Its result does not depend on the worker count either, but it is
not the serial result. On some inputs it keeps far fewer edges. Which
edges survive can differ between the schedules; the diagram never does.

The collapse records every removal. The standalone API returns a certificate
that lists each removed edge, its value, the pass (or, for the rounds
schedule, the round) it was removed in, and the witnesses that justify
it, together with the reduced graph.

An independent verifier replays the certificate. It rebuilds the graph
and checks each recorded witness directly at every scale where the
edge's neighborhood changes; between those scales the checks carry over
unchanged. For a rounds certificate it also checks that the removals of
each round are independent of each other. The verifier shares no sweep,
scheduling, or witness-selection code with the collapse. A passing
certificate has been checked twice by different means. The verifier
certifies that every recorded removal preserves the barcode and that the
reduced graph has no removable edge left. It does not certify that a run
followed any particular scheduling policy. The certificate is an
in-process value: there is no file format for it yet.

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
`collapse::collapse_sparse_ordered_parallel`. The rounds schedule is
`collapse::collapse_dense_rounds_parallel` and
`collapse::collapse_sparse_rounds_parallel`. These take a worker count as
well. The rounds schedule writes algorithm version 2 certificates, which
the verifier checks round by round. All of them return the reduced
matrix, the certificate, and run counters. The reduced graph does not
depend on the coefficient field, the homology dimension, or the thread
count, so one collapse can serve many runs.

Collapse is off by default. Finding the removable edges costs time, and
the collapse saves time only when it finds enough of them.

<!-- Break-even numbers from benchmarks/results_collapse_confirm.md (v0.4.0 release records); scaling numbers from benchmarks/results_ordered_confirm.md and benchmarks/results_rounds_confirm.md. -->

In the registered break-even study (held-out confirmation set, serial
reducer, maxdim 2), the serial collapse won end to end on the cube family
at every threshold fraction (median 2.85x, range 1.55x to 3.55x) and on
the clusters family (median 2.22x). The sphere family's median was 3.80x
over a wide range (0.62x to 6.99x: the near-full-radius entry loses). The
torus family was within noise of break-even (median 1.14x). The mode that
isolates the collapse from the sparse enumerator confirmed the gains come
from the collapse. With the reducer already on eight threads, or at maxdim
1, the collapse often costs more than it saves. The parallel schedules do
not change that picture.

In the registered scaling studies (held-out confirmation sets, four
physical cores with two threads each) the ordered schedule at four
workers ran the median headline entry at 0.84x the serial collapse speed
and the whole pipeline at 0.85x. It won end to end on three entries of
thirteen, by at most 1.13x. The rounds schedule scaled its own collapse
4.8x from one worker to eight. The whole pipeline was slower than the
serial one on every confirmed entry: it tests many more edges to reach
its fixed point, and that gap grows with the edge count. Records for
every number are attached to the release. To measure your own data, run
`benchmarks/collapse_bench.sh` in the repository's `benchmarks/`
directory. It reports edge counts, wall time, and peak memory, and it
validates the diagrams before it reports any timing.

## Correctness

Tests compare every diagram against an independent oracle (`src/oracle.rs`).
The oracle is a textbook boundary-matrix reduction over Z/p. It shares no
code with the solver, down to a different inverse algorithm. The comparison
runs on exhaustive small spaces and on randomized inputs. The tests compare
larger inputs against ripser (`RIPSER_BIN=... cargo test --test
ripser_differential`); CI pins a fixed ripser commit and also builds its
coefficient-enabled variant for `--modulus` runs. They check sparse input
against the dense engine on the same matrix and against ripser's sparse
format. A projective-plane fixture pins the torsion behavior: its H1 and H2
exist over Z/2 and vanish over Z/3. Property tests cover permutation
invariance, scaling equivariance, and the optimization toggles (clearing,
emergent pairs, apparent pairs, adjacency rows), which must not change the
diagram.

A differential gate crosses every engine setting with every storage setting.
It compares all nine combinations bar for bar against the compact dense run,
on fixtures and on randomized matrices and clouds, across thresholds,
moduli, dimensions, and thread counts, and it covers ties, absent pairs at
an infinite threshold, and disconnected components. A second gate compares
the adjacency rows against the neighbor-list test, edge by edge on tie-heavy
graphs and bar for bar on graphs and clouds.

The oracle and ripser gates cover H0, H1, and H2, over Z/2 and odd primes.
Higher dimensions run through the same generic code but are not part of that
gated claim. The parallel reducer must reproduce the serial diagram exactly.
A determinism gate recomputes random clouds, tie-heavy grids, and degenerate
fixtures at 1, 2, 4, and 8 threads over several moduli, and requires
bar-for-bar equality.

## Benchmarks

One registered study carries the engine performance of this release:
`benchmarks/north_star.sh`, which measures the shipped binary against ripser
and giotto-ph on a held-out corpus. The corpus carries the decision rule,
the noise rule, the pinning rule, the arms, the competitors, the timing
protocol, and the sampling rule, all frozen before the first measurement.
A result cannot pick its criterion afterwards. The study times fresh
processes on named physical cores and compares every diagram before it
reports a timing. A second copy of the same binary runs as an A/A control
and gives the noise band. No other script may be cited for a public
performance claim.

On the registered confirmation corpus
(`benchmarks/north_star_confirm_corpus.toml`, 31 entries with seeds disjoint
from every tuning and landing set), on one physical core of an i9-13900KS,
holos took a median 0.36 to 0.39 of the corresponding ripser build's wall
time over 24 graded headline entries. The largest ratio over all 25 graded
entries was 0.65 to 0.67. An entry whose ripser wall time is under 20 ms is
reported but not graded. By stratum: sparse-selected 0.36 to 0.38 over 25
entries, maxdim-1 0.35 to 0.41 over 20 entries, maxdim-2 0.31 over 3
entries. The dense-selected stratum had one descriptive entry and no graded
entry: with the routing rule, every dense input of the corpus above the
floor routes to the sparse engine. Public 0.5.0 took a median 1.33 to 1.35
of ripser over the same entries.

At four physical cores without SMT, holos's fresh-process wall time was a
median 0.34 to 0.37 of giotto-ph 0.2.4's in-process `ripser_parallel` time
over 17 to 18 graded headline entries. The largest ratio over all 18 to 19
graded entries was 0.58 to 0.67. The giotto-ph clock excludes process start
and input parsing, which holos includes, so the comparison favors giotto-ph.
On the CPU scaling entries holos ran 1.9 to 2.1 times faster at four cores
than at one.

The matched-precision f64 ripser build is a diagnostic reported in its own
table (29 entries against ripser-f64 and 2 against ripser-coeff-f64), never
pooled with the stock arm. The records report the A/A control for each timed
pass.

The other scripts are engineering instruments. `benchmarks/engine_bench.sh`
times holos against ripser on a tuning set and a disjoint landing set,
`benchmarks/parallel_scaling.sh` times the reduction at several thread
counts, `benchmarks/giotto_compare.sh` compares holos against giotto-ph's
`ripser_parallel` at matched thread counts, and `benchmarks/sparse_bench.sh`
times sparse input against the dense path. They have no decision rule and no
protocol gate.

Reproduce with the scripts in the repository's `benchmarks/` directory
(<https://github.com/t0rsion/holos>); `benchmarks/README.md` documents each
study. Each script writes a full provenance record (commit, binary hashes,
build flags, CPU, allowed CPUs) beside its table. The complete records are
attached to the matching GitHub release.

## License

MIT or Apache-2.0, at your option.
