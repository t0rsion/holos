# holos

[![CI](https://github.com/t0rsion/holos/actions/workflows/ci.yml/badge.svg)](https://github.com/t0rsion/holos/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/holos-tda)](https://crates.io/crates/holos-tda)
[![docs.rs](https://img.shields.io/docsrs/holos-tda)](https://docs.rs/holos-tda)
![MSRV](https://img.shields.io/crates/msrv/holos-tda)

holos computes exact Vietoris-Rips persistent homology over a prime field.
It accepts point clouds, dense distance matrices, and sparse weighted graphs.
The scalar engine uses implicit cohomology with clearing, emergent pairs,
apparent pairs, and sparse cofacet enumeration.

Selected H1 classes can produce checked circular coordinates. Finite
degree-Rips modules describe classes across scale and minimum degree. Sparse
persistence programs reuse checked reductions and repair them after supported
weight changes. Each path states its input binding and the mathematical claim
its artifact covers.

The separate `holos-tda-check` crate checks supported artifacts without
depending on the producer crate. It reconstructs reductions, class relations,
or coordinate claims from bounded records. Independent checking is not a formal
proof of the implementation.

The crates.io package is `holos-tda`, the Rust library is `holos_tda`, and the
binary is `holos`. The Python package is `holos-tda`, its import is
`holos_tda`, and its command is `holos-tda`.

## Install

```sh
cargo install holos-tda
cargo install holos-tda-check
cargo add holos-tda
pip install holos-tda
```

From a checkout:

```sh
cargo install --path crates/holos-tda
cargo install --path crates/holos-tda-check
```

## Workflows

| Task | Result | Interface |
|---|---|---|
| [Compute a diagram](#compute-a-diagram) | Exact scalar persistence | Rust, Python, CLI |
| [Explain a class](#circular-coordinates) | Source-bound class and checked circular coordinate | Rust, Python, CLI |
| [Explore scale and density](#finite-degree-rips-bipersistence) | Finite H1 module, class extensions, and checked phases | Rust, Python, CLI |
| [Update edge weights](#result-sensitive-persistence-programs) | Exact H0 and H1 updates with checked program and trace artifacts | Rust and Python; CLI compiles programs and checks artifacts |

A class record, a class at a grid node, and a program correspondence have
different contracts. The workflows below state which inputs each one accepts.

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

## Finite degree-Rips bipersistence

At scale `r`, Holos first computes each vertex degree in the threshold graph.
It keeps vertices whose degree is at least `k`, then takes the induced flag
complex. Increasing `r` and decreasing `k` gives the two parameter directions.
Degrees are computed before the induced restriction.

The `bipersistence` command accepts either every critical value or a declared
finite grid. A declared scale axis must increase and end at the input
threshold. The minimum-degree axis must decrease and end at zero.

```sh
holos bipersistence graph.spr module.hbp --format sparse \
    --scale 0.2 --scale 0.4 --scale 0.8 \
    --minimum-degree 20 --minimum-degree 10 --minimum-degree 0 \
    --rectangle 0 1 2 2 --region region.txt \
    --class-cocycle 0 1 class.cocycle --circular \
    --report module.json
holos-check module.hbp
```

Grid coordinates are zero-based indices. A region file contains one
`scale_index density_index` pair per row. The comparability graph of those
grades must be connected. The generalized rank is the rank of the canonical
map from the diagram limit to its colimit. For a product rectangle, this value
equals the map rank between its unique minimum and maximum. A connected region
without those extrema gives the nontrivial generalized-rank case.

A class atlas starts with a nonzero H1 class at one grade. At every grade in
its upper parameter cone, it records whether the affine extension fiber is
unique, ambiguous, or empty. It also partitions the cone into connected
regions with the same classification and ambiguity dimension. With
`--circular`, Holos computes a phase only at unique extensions. It leaves an
ambiguous or empty fiber without a phase.

Python exposes the same finite module:

```python
from holos_tda.bipersistence import degree_rips_bipersistence

module = degree_rips_bipersistence(
    4,
    [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
    scales=[1.0],
    minimum_degrees=[2, 0],
    modulus=47,
)
rank = module.region_rank([(0, 0), (0, 1)])
atlas = module.class_atlas((0, 0), [(0, 1)])
module.record_class_atlas((0, 0), [(0, 1)])
module.record_circular_family((0, 0), [(0, 1)])
with open("module.hbp", "wb") as output:
    output.write(module.artifact)
```

Each `(basis_index, coefficient)` term uses the canonical H1 basis at the
selected base grade. Reusing that index at another grade does not identify
the same class. The atlas records its extensions to later grades.

`BipersistenceArtifact` stores the weighted graph, grid axes, canonical H1
spaces, every cover map, and selected derived claims. The checker rebuilds the
degree-Rips slices and linear maps. It then checks squares, generalized ranks,
class atlases, and circular families. The format certifies the declared finite
grid. It does not certify a continuous module between grid values.

## Circular coordinates

`holos circular` accepts the cocycle-row format produced by Ripser.py. It
removes rows whose edges are not active at the coordinate scale before it
normalizes the cocycle.

```sh
holos circular graph.spr class.cocycle coordinate.hcc \
    --format sparse --at 0.42 --modulus 47 --phases phase.csv
holos-check coordinate.hcc
```

Each cocycle row is `u v coefficient`. The coordinate command uses an odd
prime and defaults to 47. The Rust API also accepts a caller-supplied integral
lift, including a lift for a mod-two class.

Holos can carry a class from persistence into the coordinate command without
a Ripser-shaped intermediate file:

```sh
holos graph.spr --format sparse --dim 1 --modulus 47 \
    --representatives classes.json
holos circular graph.spr classes.json coordinate.hcc --format sparse \
    --class 0 0 --phases phase.csv
holos-check coordinate.hcc
```

`--class SPACE BASIS` selects zero-based positions in the JSON written by
`--representatives`. The record supplies the field and representative scale.
The coordinate command checks the active-graph binding and consistency of
the field, interval, and class identity.

Python accepts the square distance matrix and cocycle returned by Ripser.py:

```python
from ripser import ripser
import holos_tda

result = ripser(distances, distance_matrix=True, maxdim=1,
                coeff=47, do_cocycles=True)
birth, death = result["dgms"][1][0]
scale = (birth + death) / 2
coordinate = holos_tda.circular_coordinates(
    distances,
    result["cocycles"][1][0],
    scale,
    modulus=47,
)
phase = coordinate["coordinate"]["phase"]
artifact = coordinate["artifact"]
```

`circular_points`, `circular_condensed`, and `circular_sparse` provide the
other input paths. Their `_class` variants accept records from the matching
`rips_*_classes` function. Each record binds the active labeled graph,
persistence interval, field, representative scale, and canonical class
identity. `circular_coordinates_class` is the square-matrix variant. Class
bindings require an odd prime, such as 47. Pass `other` or `other_triplets` to
compare a changed graph on the same labeled vertices. The continuation result
is `unique`, `ambiguous`, `no_extension`, or `no_nonzero_continuation`.
Holos computes a
new phase only for a unique nonzero target. An ambiguous result reports the
dimension of its additive direction.

The Rust path exposes the class, integral cocycle, field multiplier,
divisibility, gauge-fixed potential, phase, energy, and relative residual.
`CircularCoordinateArtifact` stores the semantic inputs needed for a separate
check. `holos-tda-check` rebuilds the active flag complexes, class
coordinates, lift, divisibility, residual, and continuation from the bytes.

The circular artifact covers one fixed scale. Class records additionally bind
one named persistence interval and its active source graph. The circular
checker does not verify that interval's endpoints. The artifact records
active edge endpoints, not their original weights below that scale.
Automatic lifting is a
deterministic sufficient search, not a complete lift solver. Harmonic smoothing
uses an unweighted edge objective. Exact fixed-scale H1 construction enumerates
the active triangles.

The registered circular studies compare Holos with known manifold angles,
DREiMac, an independent SciPy solve, and a public Gardner grid-cell recording.
The grid-cell run validates the software path. It makes no neuroscience claim.

## Result-sensitive persistence programs

`PersistenceProgram` compiles a sparse graph with `max_dim = 1` into checked
graph atoms. It starts from vertex-biconnected blocks and can refine a block at
a zero-filtration simplex separator. It keeps H0 global and composes H1 from
cyclic atoms. A result-sensitive guard is a filtration comparison derived from
a checked local reduction. When topology, threshold membership, and every
touched guard remain valid, `evaluate_diagram` returns exact updated H0 and H1
bars without another boundary reduction. The accepted path still scans active
edges to recompute global H0 death-edge provenance.

`advance` updates only touched atoms. When a guard fails, it repairs a
retained reduction suffix when possible, then rebuilds the atom when no
suffix is safe.
`advance_batch` applies an ordered update batch atomically. `branch` evaluates
independent alternatives from one checkpoint. Updates return their execution
mode, events, exact work counters, and class-space continuation. Exact linear
correspondence is enabled by default and can be omitted.

`ProgramArtifact` encodes a `HOLOSPRG` program bound to a labeled source graph.
`ProgramTraceArtifact` encodes a `HOLOSDLT` sequence with graph states,
checkpoints, update modes, work, events, and class relations. The independent
checker reconstructs the decomposition, nested reductions, result-sensitive
guards, accepted repairs, correspondences, and exact diagrams. It does not
certify a scheduling policy that the trace does not define.

Python compiles, updates, and exports the same program:

```python
import holos_tda

edges = [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]
updated = [(u, v, weight + 0.1) for u, v, weight in edges]
program = holos_tda.compile_sparse_program(5, edges, modulus=3)
step = program.update(5, updated)
bars = program.result()["bars"]
trace = holos_tda.compile_sparse_program_trace(5, edges, [updated], modulus=3)
with open("trace.hst", "wb") as output:
    output.write(trace)
```

The CLI compiles a program with `--program`. Program verification requires the
source graph as a second argument. The producer CLI infers the vertex count
from the largest endpoint in its triplet input. Rust and Python also accept an
explicit count, including isolated vertices. The checker source format accepts
that count on its first line, followed by `u v weight` rows. A trace contains
its graph states and needs no separate source file.

```sh
holos graph.spr --format sparse --dim 1 --program program.hsp
holos-check program.hsp graph.spr
holos-check trace.hst
```

The first two commands use a triplet-only graph with no omitted trailing
vertices. For a program exported from the Python example, its checker source
needs a first-line count of `5` to preserve vertex 4.

`holos verify-program`, `holos verify-program-trace`, and Python's
`verify_program_trace` use the producer verifier. The `holos-check` commands
above use the independent checker.

## Checked workflows

| Area | Main types or command | Checked result |
|---|---|---|
| Degree-Rips bipersistence | `BipersistenceModule`, `holos bipersistence` | Finite H1 functor, generalized ranks, class fibers, and unique-extension phases |
| Circular coordinates | `CircularCoordinateArtifact`, `holos circular` | Fixed-scale class, lift, residual, and conservative continuation |
| Collapse choice | `collapse_sparse_portfolio`, `holos collapse-portfolio` | Exact minimum over the declared schedules |
| Explicit complexes | `ExplicitReductionCertificate` | Dimension-generic `D V = R` reduction and diagram |
| Result-sensitive programs | `PersistenceProgram`, `ProgramTraceArtifact` | Exact dynamic H0 and H1 updates, local repair, and class continuation |
| Class explanation | `ExplainedDiagram`, `cohomology_space` | Canonical cocycle spaces and critical simplices |
| Reuse | `PersistenceAtlas`, `PersistenceProgram` | Checked evaluation while declared invariants hold |
| Dynamic updates | `PersistenceIndex`, `TopologyPatch`, `IndexStream` | Immutable versions, proof deltas, and exact diagrams |
| Composition | `RelativeInterfaceCertificate` | Exact cores relative to protected subcomplexes |
| Class dynamics | `KineticZigzagArtifact` | Exact affine events, relations, and zigzag intervals |
| Intervention | `CohomologyInterventionArtifact` | Minimum-cost edits for named class conditions |
| Synthesis | `SynthesisArtifact` | Minimum-cost actions over finite or complete affine states |
| Coverage | `CoverageSynthesisArtifact` | Relative fence filling under failures |
| Planar binding | `GeometryBoundCoverageArtifact` | Polygon, containment, exact radius graphs, and coverage proof |

These paths do not all make the same assurance claim. The artifact section
states each boundary.

## Exact collapse portfolios

A collapse portfolio runs every listed schedule. It replays each removal
through the collapse verifier, counts surviving flag simplices, and selects
a lexicographic minimum. Ties keep the first candidate.

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
returns a partial collapse. Every accepted removal preserves persistent
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
active subcomplex. Its basis includes both restriction kernels. The result
describes a relation between spaces, not a global identity for repeated
isomorphic interval summands.

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

The artifact formats have the verification boundaries below.
`holos-check ARTIFACT` identifies the independent formats from their magic
bytes and applies bounded decoding before allocating large collections.
Atlas, collapse, and portfolio verification use the producer crate.

| Format | Meaning | Checker boundary |
|---|---|---|
| `HOLOSBP` | Finite degree-Rips H1 module and selected claims | Independent |
| `HOLOSATL` | Source-bound scalar H1 atlas | Producer verifier; nested atlases are independently checked in programs |
| `HOLOSPRG` | Source-bound compositional H0 and H1 persistence program | Independent |
| `HOLOSDLT` | Source-bound persistence program update trace | Independent |
| `HOLOSCC` | Fixed-scale circular coordinate and continuation | Independent |
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
The check is not a proof-assistant verification. A trace check validates the
recorded state transitions and their result-sensitive conditions. It does not
certify that a producer selected a particular scheduling policy.

All portable artifacts use stable vertex labels, canonical ordering,
prime-field arithmetic, bounded decoders, explicit version bytes, resource
limits, and a SHA-256 content digest. The digest detects content changes. It
does not authenticate a producer.

## Correctness

The fast Rips engine is checked against an independent brute-force oracle and
against pinned Ripser builds. Differential tests cover dense and sparse
engines, prime fields, collapse schedules, worker counts, and optimization
toggles. The ignored release test exhausts the registered small graph space.

Proof artifacts add three checks:

1. The producer exposes verification for its artifact formats.
2. The separate checker reconstructs the accepted mathematical claim for the
   independent formats.
3. Mutation and bounded-decoder tests reject changed or malformed artifacts.

The bounded Z3 models check reduction, composition, branch coverage, and
lower-bound obligations. They also check degree-Rips monotonicity, commutative
squares, generalized ranks, affine class fibers, and the reduction guards.
These finite models are not a machine proof of the Rust implementation.

The ignored release tests enumerate small weighted graph spaces. They compare
accepted guard evaluations with fresh reduction and composed program diagrams
with monolithic reduction.

The repository enforces `#![forbid(unsafe_code)]`, warning-free rustdoc, and
no tracked Rust or Python function with McCabe complexity 11 or higher.
Functions from 6 through 10 receive a complexity review when changed.
Source-size reports compare physical and nonblank lines against a chosen
revision. The review explains necessary growth and checks whether shared
helpers preserve the right validation and mathematical boundaries.

The release gates are:

```sh
tools/check-release-hygiene.sh
tools/check-complexity.sh
tools/report-source-loc.sh BASE
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
RIPSER_BIN=/path/to/ripser RIPSER_COEFF_BIN=/path/to/ripser-coeff \
    cargo test --locked
cargo test --release --locked -- --ignored --test-threads=1
RUSTDOCFLAGS="-D warnings" cargo doc -p holos-tda --no-deps --locked
RUSTDOCFLAGS="-D warnings" cargo doc -p holos-tda-check --no-deps --locked
cargo package -p holos-tda --locked
cargo package -p holos-tda-check --locked
```

CI also checks the bounded formal models and builds the workspace with the
minimum Rust toolchain declared in the crate manifests. Package checks cover
both crates, and Python checks install the wheel and source distribution.
Replace `BASE` with the source revision being compared. Set
`SOURCE_REVIEW_BASE` to the same revision to mark changed files in the
complexity report.
Python artifact tests require `HOLOS_CHECK_BIN` and invoke that executable
for independent verification.

## Performance and studies

The frozen engine and collapse studies remain under `benchmarks/`. The
certified-workflow study records producer time, checker time, artifact bytes,
and exact work. Generated records accompany releases.

The bipersistence studies compare node ranks with multipers and GUDHI. A
Gardner grid-cell study checks selected class-extension paths. It validates the
software path without reproducing the source paper's population analysis.

The program studies cover constructed and public temporal-graph trajectories.
They record exact diagrams, reuse, repair, artifacts, and independent checks.
Diagram evaluation, class correspondence, and proof production have different
costs. A pinned external baseline checks fixed-graph H0 and H1 trajectories.
These studies describe their declared inputs and support no general speed
claim.

Every performance number in public release text must come from a generated
record. A run with a diagram mismatch is void.

## Scope and limits

- The core engine computes scalar Vietoris-Rips persistence. The finite
  multiparameter path covers H1 degree-Rips modules only.
- Holos does not implement Mapper or dedicated Cech, alpha, or cubical
  persistence engines.
- A declared degree-Rips grid says nothing about values between its axes.
  Full critical grids and exact H1 triangle enumeration can be expensive.
- Generalized-rank queries accept finite connected regions. The implementation
  does not compute presentations, resolutions, or decompositions of the
  complete module.
- Result-sensitive programs target fixed sparse graph topology and `max_dim =
  1`. A topology or threshold-membership change recompiles the program.
- Dimensions 0 and 1 remain the main performance target. Higher dimensions
  use the same bounded core but have combinatorial cost.
- Point distances and artifact coordinates are binary64 inputs. Exact
  geometry means exact reasoning about those encoded values.
- Portfolio optimality is finite. Adaptive collapse is a heuristic schedule.
- Reusable programs depend on stated structural and order invariants.
- Stable class records bind one labeled complex, interval, and canonical basis
  convention. Equal interval summands can mix under another basis convention.
- Search limits can produce an incomplete result. The bounds remain checked.
- Local durable storage is not a distributed security protocol.
- The project makes no best-in-class performance claim.

## License

Licensed under either Apache License 2.0 or the MIT License, at your option.
