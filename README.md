# holos

[![CI](https://github.com/t0rsion/holos/actions/workflows/ci.yml/badge.svg)](https://github.com/t0rsion/holos/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/holos-tda)](https://crates.io/crates/holos-tda)
[![docs.rs](https://img.shields.io/docsrs/holos-tda)](https://docs.rs/holos-tda)
![MSRV](https://img.shields.io/crates/msrv/holos-tda)

holos computes exact Vietoris-Rips persistent homology over a prime field. It
accepts point clouds, dense distance matrices, and sparse weighted graphs. The
implicit engine uses clearing, emergent pairs, apparent pairs, and sparse
cofacet enumeration.

Version 0.7 is a work in progress. It adds proof-carrying and dynamic topology
around that engine. The main new paths cover checked edge-collapse choice,
explicit filtered complexes, reusable reductions, exact class dynamics,
topological synthesis, and planar coverage.

The project favors depth over a large catalog of complexes. It does not
implement Mapper, Cech, alpha, cubical, or multiparameter persistence. The
explicit filtered-complex API is the extension point for another complex
builder. A scalar projection type does not compute a multiparameter module.

The crates.io package is `holos-tda`, the Rust library is `holos_tda`, and the
binary is `holos`. The Python package and import are both `holos-tda` and
`holos_tda`.

## Install

```sh
cargo install holos-tda
cargo add holos-tda
pip install holos-tda
```

From a checkout, use `cargo +1.92`. The plain stable alias is not used by this
repository.

```sh
cargo +1.92 install --path crates/holos-tda
```

## Compute a diagram

```sh
# Point cloud, H0 and H1.
holos points.csv

# Condensed lower-distance matrix, through H2 over Z/3.
holos data.lower --format lower-distance --dim 2 --modulus 3

# Sparse `i j distance` triplets. Unlisted pairs stay absent.
holos graph.spr --format sparse --threshold 0.5

# One worker budget covers parsing, collapse, and reduction.
holos points.csv --threads 8
```

The diagram goes to stdout. Work and routing metadata go to stderr. Input
format is inferred from common point-cloud extensions. Use `--format` to
override it. Run `holos --help` for the complete compute interface.

### Rust

```rust
use holos_tda::{DistanceMatrix, RipsParams, rips_persistence};

fn main() -> holos_tda::Result<()> {
    let points = vec![
        vec![0.0, 0.0],
        vec![1.0, 0.0],
        vec![1.0, 1.0],
        vec![0.0, 1.0],
    ];
    let distances = DistanceMatrix::from_points(&points)?;
    let diagram = rips_persistence(&distances, &RipsParams::new(1))?;
    for bar in diagram.bars {
        println!("H{}: [{}, {})", bar.dim, bar.birth, bar.death);
    }
    Ok(())
}
```

Set `RipsParams::threshold`, `modulus`, `threads`, `engine`,
`dense_storage`, `factorization`, and `collapse_schedule` for the full
compute path. Use `SparseDistanceMatrix` and `rips_persistence_sparse` for a
native sparse graph.

### Python

```python
import holos_tda

bars = holos_tda.rips_points(
    [[0, 0], [1, 0], [1, 1], [0, 1]],
    max_dim=1,
    modulus=3,
    threads=4,
)
```

`rips_condensed` and `rips_sparse` expose the matching input paths. The Python
package uses an ABI3 extension and also installs the `holos-tda` command.

## What version 0.7 adds

| Area | Main types or command | Checked result |
|---|---|---|
| Collapse choice | `collapse_sparse_portfolio`, `holos collapse-portfolio` | Exact minimum over the declared schedules |
| Explicit complexes | `ExplicitReductionCertificate` | Dimension-generic `D V = R` reduction and diagram |
| Class explanation | `ExplainedDiagram`, `cohomology_space` | Canonical cocycle spaces and critical simplices |
| Reuse | `PersistenceAtlas`, `PersistenceProgram` | Checked evaluation while declared invariants hold |
| Dynamic updates | `PersistenceIndex`, `TopologyPatch`, `IndexStream` | Immutable versions, proof deltas, and exact diagrams |
| Composition | `RelativeInterfaceCertificate` | Exact cores relative to protected subcomplexes |
| Class dynamics | `KineticZigzagArtifact` | Exact affine events, relations, and zigzag intervals |
| Intervention | `CohomologyInterventionArtifact` | Minimum-cost edits for named class conditions |
| Synthesis | `SynthesisArtifact` | Minimum-cost actions over finite or complete affine states |
| Coverage | `CoverageSynthesisArtifact` | Relative fence filling under failures |
| Planar binding | `GeometryBoundCoverageArtifact` | Polygon, containment, exact radius graphs, and coverage proof |

These paths share stable vertex labels, canonical ordering, prime-field
arithmetic, bounded decoders, and content digests. They do not all make the
same assurance claim. The artifact section states each boundary.

## Exact collapse portfolios

A collapse portfolio runs every listed schedule. It independently replays
each removal through the collapse verifier, counts surviving flag simplices,
and selects a lexicographic minimum. Ties keep the first candidate.

```sh
holos collapse-portfolio graph.spr portfolio.hpor \
    --candidate serial,rounds,adaptive \
    --score columns --dim 2 --threads 4
```

An edge score minimizes surviving edges. A column score counts every
surviving clique used by reduction through `dim + 1`, then compares counts
from the highest dimension down. The result is exact over the declared finite
portfolio. It is not a global optimum over all valid collapse sequences.

The serial and rounds schedules run to a fixed point. The adaptive schedule
ranks valid removals by downstream triangle or tetrahedron work. A work limit
returns a safe partial collapse. Every accepted removal preserves persistent
homology in all dimensions.

Python exposes the fixed serial, rounds, and adaptive portfolio:

```python
result = holos_tda.compile_collapse_portfolio(
    n,
    triplets,
    max_dim=2,
    threads=4,
    score="columns",
)
artifact = result["artifact"]
winner = result["entries"][result["selected"]]
```

## Explicit filtered complexes

`FilteredSimplicialComplex<G>` accepts simplices grouped by dimension. Every
nonempty face must occur exactly once, and a face grade must precede its
coface grade. `ScalarGrade` gives a total filtration order. `ProductGrade`
records a coordinatewise partial order and requires an explicit scalar
projection before scalar persistence.

`ExplicitReductionCertificate` records one filtration-compatible,
unit-triangular basis change per boundary dimension. The separate checker
reconstructs every face boundary, checks `D V = R`, checks unique pivots, and
derives the diagram.

```python
cycle = [([v], 0.0) for v in range(4)] + [
    ([0, 1], 1.0),
    ([1, 2], 1.0),
    ([2, 3], 1.0),
    ([0, 3], 1.0),
]
proof = holos_tda.compile_explicit_persistence(cycle, max_dim=1, modulus=3)
assert proof["artifact"].startswith(b"HOLOSEXP")
```

The implicit Rips engine remains the main performance path. Explicit
complexes provide a certified integration boundary, not a replacement for
implicit enumeration.

## Reuse, updates, and composition

The reusable APIs separate three contracts:

- `PersistenceAtlas` reuses a reduction while the stored filtration order
  stays valid. An order event triggers exact recompilation.
- `PersistenceProgram` stores a proof-carrying structural decomposition and
  repairs affected atoms.
- `PersistenceIndex` is immutable. An update returns a new root and shares
  unchanged nodes with earlier versions. `TopologyPatch` applies related
  edits atomically.

Indexes support materialized reductions and exact relative interfaces. A
relative interface fixes a protected subcomplex and stores a reduced chain
core. Composition handles disconnected pieces, certified zero-filtration
intersections, and general protected intersections. The maximum dimension is
bounded by caller limits.

`DurableInterfaceStore` is a local content-addressed store. It supports
ordered folds, restart recovery, and atomic manifest publication. It does not
provide networking, authentication, or multi-writer coordination.

## Cohomology, motion, and synthesis

`cohomology_space` computes a canonical fixed-scale basis in any accepted
dimension. `cohomology_relation` compares two spaces through their common
active subcomplex. The result describes a relation between spaces, not a
global identity for repeated isomorphic interval summands.

An affine filtration gives each edge an intercept and velocity. The event
compiler uses exact dyadic arithmetic to enumerate threshold and order
events. `KineticZigzagArtifact` builds the complete fixed-scale zigzag module
on its cells and decomposes it into intervals.

Intervention chooses weighted edge edits for named cohomology conditions.
Synthesis chooses weighted actions for rank conditions over finite states or
the complete affine event schedule. Both can return `Optimal`, `Infeasible`,
or `SearchIncomplete`. An incomplete result carries checked bounds and any
feasible incumbent.

The proof tree certifies lower bounds and branch coverage. The checker does
not rerun the producer's branch-and-bound search.

## Geometry-bound coverage

The graph-level coverage path checks whether a canonical fence cycle bounds
a two-chain in the active Rips complex. It quantifies over sensor failures and
can synthesize a minimum-cost activation plan.

For a physical finite-state claim, pass one two-dimensional point file per
state:

```sh
holos cover coverage.hgeo \
    --state state-0.spr --coordinates state-0.pts \
    --vertices 5 --broadcast-radius 2 --sensing-radius 2 \
    --fence 0,1,2,3 --candidate 4 1 all --max-activations 1

holos-check coverage.hgeo
```

The `HOLOSGEO` checker treats each finite binary64 coordinate as an exact
dyadic rational. It checks that the fence is a simple nondegenerate polygon,
that every sensor lies in or on it, and that each declared communication
state is the complete Euclidean broadcast-radius graph. It also checks the
radius inequality, relative chains, failure cases, and optimization proof.

Python provides the same finite profile:

```python
plan = holos_tda.synthesize_geometric_coverage(
    5,
    [state_edges],
    [[(0, 0), (2, 0), (2, 2), (0, 2), (1, 1)]],
    [0, 1, 2, 3],
    [(4, 1)],
    broadcast_radius=2,
    sensing_radius=2,
    max_activations=1,
)
```

Affine coverage proves completeness of a graph threshold schedule. It does
not prove that the affine edge weights have a Euclidean realization.

## Artifacts and the checker

`holos-check ARTIFACT` identifies these formats from their magic bytes and
applies bounded decoding before it allocates large collections.

| Format | Meaning | Checker boundary |
|---|---|---|
| `HOLOSPF` | Sparse persistence proof DAG | Independent |
| `HOLOSEXP` | Explicit filtered-complex reduction | Independent |
| `HOLOSZZ` | Kinetic cohomology zigzag | Independent |
| `HOLOSSYN` | Weighted topology synthesis | Independent |
| `HOLOSCI` | Named-class intervention | Independent |
| `HOLOSCOV` | Failure-tolerant relative coverage | Independent |
| `HOLOSGEO` | Geometry-bound coverage | Independent |
| `HOLOSRI` | Relative filtered interface | Independent |
| `HOLOSDM` | Distributed interface manifest | Independent with object callback |
| `HOLOSIP`, `HOLOSDP` | Index snapshot and delta | Independent |
| `HOLOSCOL`, `HOLOSPOR` | Collapse and portfolio | Linked collapse verifier |

Independent means that `holos-tda-check` does not depend on `holos-tda`.
The two crates still share the mathematical specification and byte formats.
This is an implementation-independent replay check, not a proof-assistant
verification.

All portable artifacts use canonical ordering, explicit version bytes,
resource limits, and a SHA-256 content digest. The digest detects content
changes. It does not authenticate a producer.

## Correctness

The fast Rips engine is checked against an independent brute-force oracle and
against pinned Ripser builds. Differential tests cover dense and sparse
engines, prime fields, collapse schedules, worker counts, and optimization
toggles. The ignored release test exhausts the registered small graph space.

Proof artifacts add three checks:

1. The producer verifies its artifact before returning it.
2. The separate checker reconstructs the accepted mathematical claim for the
   independent formats.
3. Mutation and bounded-decoder tests reject changed or malformed artifacts.

The `formal/v07` Z3 suite checks finite logical obligations for reduction,
portfolio selection, relative boundaries, maximal failures, branch
partitions, component composition, and lower bounds. These obligations are
not a machine proof of the Rust implementation.

The repository enforces `#![forbid(unsafe_code)]`, warning-free rustdoc, and
no tracked Rust or Python function with McCabe complexity 11 or higher.

The release gates are:

```sh
cargo +1.92 fmt --all -- --check
cargo +1.92 clippy --all-targets --locked -- -D warnings
RIPSER_BIN=/path/to/ripser cargo +1.92 test --locked
cargo +1.92 test --release --locked -- --ignored
RUSTDOCFLAGS="-D warnings" cargo +1.92 doc -p holos-tda --no-deps --locked
cargo +1.92 package -p holos-tda --locked
formal/v07/check.sh
```

## Performance and studies

The public 0.6 engine study remains the evidence for the core Rips engine. Its
frozen corpus, protocol, and runners remain under `benchmarks/`. The release
records are attached to that GitHub release.

Version 0.7 adds `benchmarks/research_bench.sh`. It records producer time,
checker time, artifact bytes, and exact work for the portfolio,
explicit-complex, and geometry-bound coverage paths. The current constructed
cases are integration gates. They do not support a general speed or novelty
claim.

Every performance number in public release text must come from a generated
record. A run with a diagram mismatch is void.

## Scope and limits

- The core engine computes scalar Vietoris-Rips persistence. It is not a
  general TDA pipeline.
- Dimensions 0 and 1 remain the main performance target. Higher dimensions
  use the same bounded core but have combinatorial cost.
- Point distances and artifact coordinates are binary64 inputs. Exact
  geometry means exact reasoning about those encoded values.
- Portfolio optimality is finite. Adaptive collapse is a heuristic schedule.
- Reusable programs depend on stated structural and order invariants.
- Stable class records are canonical for one labeled complex and convention.
  Equal interval summands can mix under another basis convention.
- Search limits can produce an incomplete result. The bounds remain checked.
- Local durable storage is not a distributed security protocol.
- The project makes no best-in-class claim for the new v0.7 workflows.

## License

Licensed under either Apache License 2.0 or the MIT License, at your option.
