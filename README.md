# holos

[![CI](https://github.com/t0rsion/holos/actions/workflows/ci.yml/badge.svg)](https://github.com/t0rsion/holos/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/holos-tda)](https://crates.io/crates/holos-tda)
[![docs.rs](https://img.shields.io/docsrs/holos-tda)](https://docs.rs/holos-tda)
![MSRV](https://img.shields.io/crates/msrv/holos-tda)

holos computes Vietoris-Rips persistent homology. It produces exact
barcodes over a prime field Z/p, with Z/2 as the default. It reads point
clouds and dense or sparse distance matrices. The engine is implicit, in
the same class as [ripser](https://github.com/Ripser/ripser). An
independent oracle and ripser itself check every diagram in the test suite.
It can also compute canonical fixed-scale cohomology in any bounded dimension
and exact maps through labeled subcomplex inclusions. Affine edge
trajectories have exact event schedules and complete fixed-scale zigzag
decompositions. Exact event complexes separate simultaneous births and
deaths without assigning a false class identity. A weighted search can
produce one independently checked minimum-cost edge intervention across
several declared graph scenarios. Proof-carrying synthesis extends this to
canonical cohomology subspaces, state-specific actions, and exact affine time.
Its checker validates an optimality tree without repeating the producer
search. A separate relative-coverage path finds minimum-cost sensor
activations across communication states and bounded failures. Its physical
conclusion is conditional on the controlled-boundary domain and placement
assumptions. The persistence explain profile separately returns canonical H1
class spaces and can compile an immutable persistence index for a sparse
graph. The index maintains every requested
homology dimension as weights change inside a fixed listed-edge envelope. It
composes relative filtered cores through arbitrary separators, including
separators with nonzero homology. Descendants retain every separator shared
with an ancestor. Updates path-copy changed routes and share untouched
subtrees. Cold checkpoints and warm proof deltas let the `holos-check` binary
verify a mixed version stream without linking to the solver. Relative cores
can also move through a local content-addressed store. Ordered
folds survive interruption, and the independent checker reloads proof objects
by content id without retaining the complete object set. The Rust crate is
[`holos-tda`](https://crates.io/crates/holos-tda)
(library path `holos_tda`, binary `holos`). The Python package is
[`holos-tda`](https://pypi.org/project/holos-tda/) (import `holos_tda`).

## Status

Work in progress. The release gates cover ordinary persistence through H2 and
graded index persistence through H3. Higher dimensions run through the same
dimension-generic code but do not carry the same testing claim. A run is
serial by default. `--threads` is the worker budget for the whole run: the
input parse, the reduction, and, with a parallel collapse schedule selected,
the edge collapse. The diagram is identical at any thread count. See
"Correctness" for what the release gates cover.

The ordinary command uses the compute profile. It keeps every routing and
reduction fast path. `--representatives` selects the explain profile, which
returns canonical H1 class spaces and cocycles. `--atlas` adds critical
simplices and an independent explicit reduction certificate for one complete
weak edge order. `--program` compiles the result-sensitive compositional
model. `holos index` compiles versioned separator interfaces through the
dimension selected by `--dim`. Persistent class spaces and dynamic index
correspondence remain H1-only. The separate fixed-scale cohomology API is
dimension-generic. Kinetic zigzags use the same explicit path and decompose
over any supported prime field. Synthesis constrains canonical subspaces in
any supported dimension. An affine source binds the finite specification to
the complete fixed-scale threshold schedule. Producing any proof can cost
more than the implicit compute path. Relative coverage uses a planar fence
and a two-chain over any supported prime field. The graph, fence, radius
inequality, and chain are checked. You must supply the planar domain, sensor
positions, boundary map, and communication facts required by the
controlled-boundary theorem.

## Install

```sh
cargo install holos-tda          # CLI (binary is named `holos`)
cargo install holos-tda-check    # independent proof checker (`holos-check`)
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

# Keep the subcomplex on vertices 0 through 3 fixed, cancel the remaining
# chain complex, and write an independently checked interface certificate:
holos interface graph.spr interface.hri --format sparse --dim 2 \
  --protect 0 --protect 1 --protect 2 --protect 3

# Store ordered child interfaces, resume any durable prefix, and publish a
# manifest plus the composed certificate:
holos merge-interfaces interface-store job.hdm result.hri \
  child-a.hri child-b.hri \
  --separator 0 --separator 1 --separator 2 --separator 3

# Compute canonical H2 at scale 1.0 and relate it to another active graph:
holos cohomology old.spr new.spr --format sparse \
  --at 1.0 --homology-dim 2 --modulus 5

# Read affine edges as "u v intercept velocity". Decompose the complete
# fixed-scale H2 event zigzag and write its self-contained proof:
holos kinetic edges.kin --vertices 6 --start 0 --end 1 \
  --at 1.0 --homology-dim 2 --modulus 3 --zigzag run.hzz

# Reconstruct the event schedule, cohomology maps, and decomposition in the
# separate checker:
holos-check run.hzz

# Search weighted absent edges. Target 0 is an index in the canonical basis
# printed by `holos cohomology`:
holos intervene-cohomology graph.spr edit.hci --format sparse \
  --at 1.0 --homology-dim 2 --target 0 --max-edits 2 \
  --candidate 0 1 5 --candidate 2 3 8

# Find one minimum-cost link set that kills the named class in every sparse
# scenario. Each target corresponds to the scenario at the same position:
holos plan-links plan.hci --vertices 12 \
  --scenario normal.spr --target 0 \
  --scenario failed-link.spr --target 1 \
  --at 1.0 --homology-dim 1 --modulus 3 --max-edits 3 \
  --candidate 0 4 7 --candidate 2 8 11

# Check the self-contained intervention in the separate process:
holos-check edit.hci

# Minimize cost while reducing the complete H1 rank to zero in every listed
# state. The state files use sparse "i j d" rows:
holos synthesize plan.hsyn \
  --state morning.spr --state evening.spr --vertices 20 \
  --at 1.0 --homology-dim 1 --max-rank 0 --modulus 3 \
  --max-edits 3 --candidate 0 4 7 --candidate 2 8 11

# Bind the same rank condition to every time in an affine trajectory. Input
# rows are "u v intercept velocity":
holos synthesize-kinetic motion.kin plan.hsyn \
  --vertices 20 --start 0 --end 10 --at 1.0 \
  --homology-dim 1 --max-rank 0 --modulus 3 --max-edits 3 \
  --candidate 0 4 7 --candidate 2 8 11

# Reconstruct the finite or affine source, all ranks, and the proof tree:
holos-check plan.hsyn

# Activate two candidate sensors in one or more listed communication states.
# The fence is always active. Candidate support is `all` or state indices:
holos cover coverage.hcov --vertices 6 \
  --state normal.spr --state degraded.spr \
  --broadcast-radius 1.0 --sensing-radius 1.0 --fence 0,1,2,3 \
  --failable 4,5 --failure-budget 1 --max-activations 2 \
  --candidate 4 2 all --candidate 5 3 0,1 --modulus 3

# Bind the same failure-tolerant criterion to every graph in the complete
# threshold schedule of affine rows `u v intercept velocity`:
holos cover-affine motion.kin coverage.hcov --vertices 6 \
  --start 0 --end 10 --broadcast-radius 1.0 --sensing-radius 1.0 \
  --fence 0,1,2,3 --failable 4,5 --failure-budget 1 \
  --max-activations 2 --candidate 4 2 all --candidate 5 3 all

# Rebuild the affine schedule, every failure check, and the optimality tree:
holos-check coverage.hcov

# Parallel parse and reduction with 8 worker threads (same diagram as
# serial):
holos points.csv --threads 8

# Force the engine for a dense input (auto is the default):
holos data.lower --format lower-distance --engine dense

# Forbid the full row-major matrix on a dense run (auto is the default):
holos data.lower --format lower-distance --dense-storage compact

# Build identity (version, git commit, profile):
holos --version

# Adaptive collapse for an H2 run, with a portable certificate:
holos points.csv --dim 2 --collapse-edges --collapse-schedule adaptive \
  --collapse-certificate points.hcol

# Verify that artifact against the original input in a separate process:
holos verify-collapse points.csv points.hcol

# Write stable H1 class spaces and a proof-carrying persistence atlas:
holos points.csv --threshold 0.25 --representatives classes.json \
  --atlas points.hatlas

# Check the graph binding, reduction proof, diagram, and cocycles without the
# persistence solver:
holos verify-atlas points.csv points.hatlas

# Check a self-contained trajectory with proofs at every region boundary:
holos verify-trajectory run.htrace

# Compile a result-sensitive program over checked sparse graph atoms:
holos graph.spr --format sparse --dim 1 --program graph.hprogram

# Check its graph binding, decomposition, atom proofs, and composed diagram:
holos verify-program graph.spr graph.hprogram --format sparse

# Produce and check a restricted finite H1 intervention. Space 0 is the
# first space in the program result:
holos intervene graph.spr graph.hprogram edit.hint --format sparse \
  --space 0 --before 0.2
holos verify-intervention edit.hint

# Check a self-contained program update trace:
holos verify-program-trace run.hdelta

# Build one proof DAG for an initial sparse graph and two later states, then
# check it with the separate solver-independent binary:
holos prove initial.spr run.hpf update-1.spr update-2.spr --format sparse
holos-check run.hpf

# Build one initial index checkpoint and two ordered proof records:
holos index initial.spr initial.hip --format sparse \
  --dim 2 \
  --update update-1.spr --record update-1.hdp \
  --update update-2.spr --record update-2.hdp

# Verify the stream. An envelope-changing record is a new checkpoint:
holos-check initial.hip update-1.hdp update-2.hdp

# Enable automatic structural factorization for a sparse run:
holos graph.spr --format sparse --factorization auto
```

`--engine` picks the engine for a dense input, and `--dense-storage` picks
the form that engine reduces from. See "Engine" for both rules. The diagram
is the same under every setting.

The diagram goes to stdout, and computation metadata goes to stderr.

## Library

```rust
use holos_tda::{PointCloudGraph, PointCloudParams, RipsParams};

fn main() -> holos_tda::Result<()> {
    let points = vec![
        vec![0.0, 0.0],
        vec![1.0, 0.0],
        vec![1.0, 1.0],
        vec![0.0, 1.0],
    ];
    let graph = PointCloudGraph::build(&points, PointCloudParams::new(1.1))?;
    let params = RipsParams::new(1).with_threshold(1.1);
    let diagram = holos_tda::rips_persistence_sparse(graph.matrix(), &params)?;
    for bar in &diagram.bars {
        println!("dim {}: [{}, {})", bar.dim, bar.birth, bar.death);
    }
    Ok(())
}
```

`RipsParams::with_modulus(p)` switches the coefficient field.

`cohomology_space` computes `H^q` of the active flag complex at one scale. It
returns canonical cocycle vectors, stable content ids, simplex counts, and a
rank. `cohomology_relation` restricts two such spaces to their common active
subcomplex and returns the exact intersection of the two restriction images.
The inputs must share their vertex set, scale bits, dimension, and field.

`KineticFiltration` treats each affine `f64` coefficient as an exact dyadic
rational. It enumerates all interior edge-order equalities and optional
threshold crossings. `cohomology_events` relates the fixed-scale spaces on
the open cells before and after each exact event. Its float bounds enclose the
exact rational root.

`cohomology_zigzag` alternates those open cells with the flag complex at each
exact event. Continuity makes both adjacent complexes subcomplexes of the
event complex. The induced cohomology restrictions form a finite type-A
zigzag. `ZigzagModule::decompose` computes every generalized rank and applies
Möbius inversion to recover exact interval multiplicities. Equal interval
copies form one isotypic space. The API does not assign separate identities
inside that space.

`CohomologySubspace` stores a unique reduced coordinate basis inside one
`CohomologySpace`. A `SynthesisState` bounds the dimension of the intersection
between that subspace and the restriction image from an edited flag complex.
The number is independent of every input basis. `SynthesisAction` adds one
edge in a declared subset of states and has a positive integer cost.

`SynthesisArtifact::build` minimizes total cost under `max_edits`. `Optimal`
and `Infeasible` results carry a complete proof tree. `SearchIncomplete`
carries a checked incumbent and lower bound when the limited search found
them. `TopologicalSpecification::components` returns the exact connected
components of the state-action incidence relation. The global edit limit can
still couple their cost and cardinality choices.

`TopologicalSpecification::from_kinetic_rank_ceiling` compiles an affine
trajectory into endpoints, threshold events, and open time cells. Its
`SynthesisSource::Affine` record binds the trajectory to the artifact. The
separate checker reconstructs the complete schedule, including states that
were omitted because they already met the rank ceiling.

`HOLOSSYN` version 1 stores the source, specification, actions, result, and
optimality tree. The separate checker rebuilds every flag complex,
cohomology space, restriction image, subspace intersection, and proof rule.
It does not run the producer's branch-and-bound search.

`evaluate_planar_coverage` checks whether the canonical `CoverageFence`
cycle bounds a two-chain in the active flag complex. `PlanarCoverageModel`
checks `3 * sensing_radius^2 >= broadcast_radius^2` on the exact dyadic
values. Under the controlled-boundary assumptions, the resulting relative
class is a sufficient physical coverage certificate. The API cannot infer
the domain, sensor positions, or boundary map from the graph.

`CoverageSynthesisArtifact::build` minimizes positive integer activation
cost across every `CoverageState` and every failure set up to the declared
budget. Activation monotonicity makes maximal failure sets sufficient. The
state-action incidence graph yields exact `CoverageComponentFrontier` values,
which compose under one global activation limit. An affine
`CoverageSpecification` stores the complete fixed-radius threshold schedule
of its `KineticFiltration` source.

`HOLOSCOV` version 1 stores the geometric contract, source, states, actions,
failures, result, and proof tree. The separate checker has its own affine
compiler, finite-field boundary solver, failure enumeration, and proof
interpreter. It checks optimality without repeating producer search.

`CohomologyInterventionArtifact` finds one weighted edge set across declared
graph scenarios. A named class survives exactly when it lies in the
restriction image from the edited cohomology space. Adding more edges cannot
make a killed class return. The search uses this fact to derive necessary
candidate sets, pack disjoint sets into a cost lower bound, and branch only
where every feasible plan must branch. `Optimal` means minimum total cost
over the declared candidates under `max_edits`. A limited search can return a
checked feasible plan and lower bound with `SearchIncomplete`.

`HOLOSCI` version 2 stores the scenarios, positive integer costs, selected
edges, work limits, bounds, and disjoint root necessary sets. The separate
`holos-tda-check` crate reconstructs every cohomology space and restriction
map, then repeats the search without linking to the producer crate.

These APIs describe fixed-scale cohomology. They do not assign identities to
persistence intervals or homology cycles. Explicit flag enumeration and
weighted candidate search can be exponential. A Rips rank condition does not
by itself certify physical sensor coverage.

`RipsParams::threads` is the worker budget; 1 means serial.
`RipsParams::with_engine` and `RipsParams::with_dense_storage` take the
same choices as `--engine` and `--dense-storage`.
`RipsParams::with_factorization` takes the same choices as
`--factorization`.
For an unthresholded point run, build a `DistanceMatrix` and call
`rips_persistence`. For sparse input, use
`SparseDistanceMatrix::from_triplets` with `rips_persistence_sparse`. To
read a file, use `holos_tda::io::read_point_cloud`,
`read_lower_distance_matrix`, or `read_sparse_matrix`; each takes the path
and a worker budget. To parse text you already hold, use
`io::parse_point_cloud`, `io::parse_condensed`, or `io::parse_triplets`.

Compile a sparse graph when you will evaluate several weight updates:

```rust
use holos_tda::{CertificateLimits, RipsParams, SparseDistanceMatrix};

let params = RipsParams::new(1).with_modulus(3);
let graph = SparseDistanceMatrix::from_triplets(
    4,
    &[(0, 1, 1.0), (0, 3, 1.1), (1, 2, 1.2), (2, 3, 1.3)],
)?;
let (artifact, atlas) = holos_tda::AtlasArtifact::compile(
    &graph,
    &params,
    CertificateLimits::default(),
)?;
let proof_bytes = artifact.encode()?;

// `updated` has the same weak edge-weight order as `graph`.
let updated = SparseDistanceMatrix::from_triplets(
    4,
    &[(0, 1, 1.01), (0, 3, 1.11), (1, 2, 1.21), (2, 3, 1.31)],
)?;
let diagram = atlas.evaluate_diagram(&updated)?;
let explained = atlas.evaluate(&updated)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`evaluate_diagram` returns only H0 and H1. `evaluate` also returns class
spaces, critical pairs, lineages, and edge-weight sensitivities. Call
`events` before an update when you need the exact reason a region ended.
`update` performs exact fallback reduction after an event.

Use an index for repeated changes inside one listed-edge envelope:

```rust
use holos_tda::{
    CertificateLimits, CorrespondenceMode, IndexEdit, IndexParams,
    IndexStream, PersistenceIndex, RipsParams, TopologyPatch,
};

# let graph = holos_tda::SparseDistanceMatrix::from_triplets(
#     4,
#     &[(0, 1, 1.0), (1, 2, 1.1), (2, 3, 1.2), (0, 3, 1.3)],
# )?;
let mut params = RipsParams::new(2).with_modulus(3);
params.threshold = Some(2.0);
let index = PersistenceIndex::compile(
    &graph,
    &params,
    IndexParams::default(),
    CertificateLimits::default(),
)?;
let mut stream = IndexStream::new(index);
let snapshot = stream.checkpoint()?.encode()?;

let patch = TopologyPatch::new(vec![IndexEdit::set_weight(0, 1, 1.05)]);
let update = stream.apply_patch(&patch, CorrespondenceMode::Exact)?;
let record = update.proof.encode()?;
println!(
    "{} shared nodes, {} checkpoint bytes, {} record bytes",
    update.transition.work.nodes_shared,
    snapshot.len(),
    record.len(),
);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`IndexStream` emits a warm delta inside one listed-edge envelope. It emits a
cold checkpoint when the envelope changes. `apply_patches` commits an ordered
patch batch atomically. At the index level, `transition` omits cross-version
class relations. `transition_with` computes them only when you select
`CorrespondenceMode::Exact`. `branch` evaluates alternatives in input order
and uses `RipsParams::threads` as its worker budget. `diff` reports exact
diagram changes and physically shared tree nodes. `explain` computes
canonical H1 class spaces on demand.

Use a program when you need result-sensitive guards or restricted H1
interventions:

```rust
use holos_tda::{
    CertificateLimits, CorrespondenceMode, PersistenceProgram, RipsParams,
};

# let graph = holos_tda::SparseDistanceMatrix::from_triplets(
#     4,
#     &[(0, 1, 1.0), (1, 2, 1.1), (2, 3, 1.2), (0, 3, 1.3)],
# )?;
let params = RipsParams::new(1).with_modulus(3);
let mut program = PersistenceProgram::compile(
    &graph,
    &params,
    CertificateLimits::default(),
)?;
let proof = holos_tda::ProgramArtifact::from_program(&program)?.encode()?;

# let changed = graph.clone();
let update = program.advance(&changed)?;
println!("{:?}: {} atoms rebuilt", update.mode, update.work.atoms_rebuilt);
for relation in &update.correspondence {
    println!("relation rank: {}", relation.relation_rank);
}

// State-only maintenance omits cross-state correspondence.
let state_update = program.advance_with(&changed, CorrespondenceMode::Omit)?;
assert!(state_update.correspondence.is_empty());

// A batch commits only when every step succeeds. Branches start from one
// immutable checkpoint and preserve alternative order.
let checkpoint = program.checkpoint();
let batch = program.advance_batch(&[changed.clone()])?;
program.restore(&checkpoint);
let branches = checkpoint.branch(&[changed.clone()])?;
let proof = holos_tda::ProofArtifact::build(
    &graph,
    &[changed],
    &params,
    CertificateLimits::default(),
)?.encode()?;
println!("{} updates, {} branches, {} proof bytes", batch.len(), branches.len(), proof.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

`evaluate_diagram` runs no persistence reduction when every touched atom
passes its result-sensitive guards. `advance` returns `Reused`, `Repaired`,
or `Recompiled`. It records exact work, events, continuation, and exact
relations on common filtered subcomplexes. `CorrespondenceMode::Omit` keeps
the exact current result but skips cross-state relations. `advance_batch_with`
and `branch_with` apply the same choice to batches and alternatives.

`ProgramTraceArtifact` writes self-contained `HOLOSDLT` trajectories.
`kill_h1_before` implements one restricted finite H1 intervention. Its
`Optimal` status is relative to the current checked reduction and declared
destroyer triangles. It is not a global inverse-persistence claim.

`PointPersistenceAtlas` applies the same model to Euclidean points at a
finite threshold. `coordinate_radius` is a conservative per-point
displacement bound. `sensitivities` returns analytic coordinate derivatives
for untied finite endpoints.

## Python

```python
import holos_tda

points = [[0, 0], [1, 0], [1, 1], [0, 1]]
bars = holos_tda.rips_points(points, max_dim=1, threshold=1.5)
# [(0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, 1.0), (0, 0.0, inf), (1, 1.0, 1.4142...)]

bars, classes = holos_tda.rips_points_classes(points, max_dim=1)
# classes[0]["terms"] contains (u, v, coefficient) triples.

edges = [(0, 1, 1.0), (0, 3, 1.1), (1, 2, 1.2), (2, 3, 1.3)]
atlas = holos_tda.compile_sparse_atlas(4, edges, modulus=3)
result = atlas.evaluate(
    4,
    [(u, v, distance + 0.01) for u, v, distance in edges],
)
proof_bytes = atlas.artifact

point_atlas = holos_tda.compile_points_atlas(points, threshold=1.5)
radius = point_atlas.coordinate_radius
coordinate_gradients = point_atlas.sensitivities()

program = holos_tda.compile_sparse_program(4, edges, modulus=3)
update = program.update(
    4,
    [(u, v, distance + 0.01) for u, v, distance in edges],
)
work = update["work"]
continuation = update["continuation"]
correspondence = update["correspondence"]
program_bytes = program.artifact
current_proof = program.proof

state_only = program.update(4, edges, correspondence=False)
branches = program.fork(4, [edges, edges])

trace_bytes = holos_tda.compile_sparse_program_trace(
    4, edges, [[(u, v, distance + 0.01) for u, v, distance in edges]],
    modulus=3,
)
checked = holos_tda.verify_program_trace(trace_bytes)
proof_bytes = holos_tda.compile_sparse_proof(4, edges, [edges], modulus=3)

index = holos_tda.compile_sparse_index(4, edges, max_dim=2, modulus=3)
snapshot_bytes = index.snapshot
update = index.update(
    4,
    [(u, v, distance + 0.01) for u, v, distance in edges],
    correspondence=False,
)
delta_bytes = update["delta_proof"]
patch = index.patch([("set", 0, 1, 1.05)], correspondence=False)
branches = index.fork(4, [edges, edges])

interface = holos_tda.compile_relative_interface(
    4, edges, protected=[0, 1, 2, 3], max_dim=2, modulus=3,
)
commit = holos_tda.merge_relative_interfaces(
    [interface["artifact"]], "interface-store", protected=[0, 1, 2, 3],
)
manifest_bytes = commit["manifest"]

cycle = [(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)]
space = holos_tda.cohomology_space(4, cycle, dimension=1, scale=1.0)
relation = holos_tda.cohomology_relation(
    4, cycle, cycle, dimension=1, scale=1.0,
)
events = holos_tda.affine_events(
    4,
    [(u, v, value, 0.0) for u, v, value in cycle] + [(0, 2, 2.0, -2.0)],
    0.0, 1.0, threshold=1.0, dimension=1,
)
zigzag = holos_tda.kinetic_zigzag(
    4,
    [(u, v, value, 0.0) for u, v, value in cycle] +
    [(0, 2, 2.0, -2.0)],
    0.0, 1.0, dimension=1, scale=1.0,
)
intervention = holos_tda.intervene_cohomology(
    4, [(cycle, 0)], [(0, 2, 7)], dimension=1, scale=1.0, max_edits=1,
)
synthesis = holos_tda.synthesize_cohomology(
    4, [cycle, cycle], [(0, 2, 7)],
    dimension=1, scale=1.0, max_rank=0, max_edits=1,
)
affine_synthesis = holos_tda.synthesize_affine_cohomology(
    4,
    [(u, v, value, 0.0) for u, v, value in cycle] +
    [(0, 2, 2.0, -2.0)],
    0.0, 1.0, [(1, 3, 5)],
    dimension=1, scale=1.0, max_rank=0, max_edits=1,
)

fenced_wheel = [
    (0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0),
    (0, 4, 1.0), (1, 4, 1.0), (2, 4, 1.0), (3, 4, 1.0),
    (0, 5, 1.0), (1, 5, 1.0), (2, 5, 1.0), (3, 5, 1.0),
]
coverage = holos_tda.relative_coverage(
    6, fenced_wheel, [0, 1, 2, 3, 4], [0, 1, 2, 3], 1.0, 1.0,
)
coverage_plan = holos_tda.synthesize_coverage(
    6, [fenced_wheel], [0, 1, 2, 3], [(4, 2), (5, 3)],
    1.0, 1.0, 2, failable=[4, 5], failure_budget=1,
)
```

`rips_condensed` and `rips_sparse` mirror the Rust entry points. All
three accept `max_dim`, `threshold`, `modulus`, `threads`, `factorization`,
`collapse_edges`, `collapse_schedule`, `collapse_objective`, and
`collapse_work_limit`. They select the engine and the storage form
automatically. The `_classes` variants return `(bars, classes)` for each
input form. The `holos-tda` script is the same CLI as the Rust binary, so it
carries every flag.

`SparseAtlas.update` reports `reused` inside a region and `recomputed` after
an event. A recomputed update also replaces `artifact` with the new region's
proof. `load_sparse_atlas` checks saved `HOLOSATL` bytes before it constructs
the Python object. `PointAtlas` exposes the same reuse and fallback model for
finite-threshold Euclidean point clouds.

`SparseProgram` exposes atoms, exact work, events, continuation, and exact
class correspondence. `update_many` is atomic. `fork` returns independent
programs in alternative order. Set `correspondence=False` for state-only
maintenance. `load_sparse_program` checks saved `HOLOSPRG` bytes.
`SparseProgram.intervene` returns a feasible edit and optional `HOLOSINT`
bytes. `verify_intervention` checks those bytes without the persistence
solver.

`SparseIndex` exposes the current content identifier, diagram, separator
interfaces, cold checkpoint, exact work, events, and diagram delta. Its
`update_many` method is atomic. Its `fork` method returns independent indexes.
`patch` applies `set`, `activate`, and `deactivate` edits as one transaction.
An envelope-changing update returns a replacement cold checkpoint instead of
a warm delta.

The optional `holos_tda.torch` module supplies `StrictSparseProgram` and
`finite_h1_intervals`. It differentiates finite H1 birth and death endpoints
with respect to listed edge weights. It requires fixed unique edges, distinct
finite weights, and no weight equal to the threshold. It raises at a tie
instead of selecting a gradient.

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
boundaries. A text under one mebibyte parses serially whatever you set. Each
chunk knows the line number it starts at, so the values are the same bit for
bit and an error names the same line as a serial parse.

## Engine

holos has two engines, and both feed the same reduction. Neither stores a
boundary matrix: they enumerate cofacets on demand. The dense engine reads a
distance matrix and scans the vertex range for each cofacet. The sparse
engine reads a graph and merges neighbor lists. Sparse input always uses the
sparse engine. For dense input, routing picks one.

### Point-cloud construction

With an explicit finite threshold, a point-cloud run builds the exact sparse
graph directly. It does not allocate the condensed or full distance matrix.
Automatic construction uses a k-d-tree radius join through 12 coordinates.
It tests pairs in exhaustive blocks above that dimension and at an infinite
threshold. Both kernels use the same scaled Euclidean distance calculation.
Their edge values are bit-identical, and their output does not depend on the
worker count.

`PointCloudGraph::build` exposes both kernels and returns the selected
`PointCloudStrategy`, distance evaluation count, and edge count. The CLI
prints those counters to stderr. A point run without an explicit threshold
still builds a dense matrix because it must first find the enclosing radius.

### Routing

`--engine auto`, the default, counts the edges of a dense input at the
resolved threshold and reduces a low-density input as that thresholded
graph. The rule is frozen and takes no argument. The input needs at least 32
points, the edge density must be at or below four fifths (`m / C(n, 2)`,
with `m` the finite pairs at or below the threshold), and the conversion,
at 24 bytes an edge and 24 a point, must fit a memory budget of 32 MiB, or
the bytes of the compact matrix, whichever is larger. An infinite threshold routes as well, because it admits every finite
pair and no absent one, so a matrix of mostly absent pairs still has a
sparse graph. `--engine dense` keeps the matrix. `--engine sparse` converts
and ignores the budget, because it is an explicit request. A sparse input is
never routed, and neither is a collapse run: the collapse already hands the
sparse engine a graph. The diagram is identical under every setting.
On the registered confirmation corpus the median wall-time ratio of holos
over the corresponding ripser build on one physical core was sparse-selected
0.37 over 25 entries (see Benchmarks).

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
so a run in the full form holds one and a half times that form. The diagram
is identical under every setting. The peak RSS of every arm and competitor is in the
record's "Peak RSS" table.

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
their own work estimate into a worker count under that budget, and a step
with little work runs on one thread. The constants are frozen, and a
decision table in the tests pins them: the dim-0 apparent test takes a
second worker at 128 cycle edges, the assembly at 256 source simplices, and
the sort at 20,000 elements. The diagram, the column order, and the pivot
registry are the same at every worker count. On the CPU scaling entries of
the registered confirmation corpus holos ran 1.9 to 2.0 times faster at four
physical cores than at one (see Benchmarks).

### Structural factorization

The sparse engine can split positive-dimensional persistence across the
vertex-biconnected blocks of the graph at the terminal level. Every clique
with at least two vertices belongs to one such block. The engine computes H0
once on the whole graph, reduces each cyclic block for dimensions above zero,
and merges the bars in canonical order. Independent blocks share the worker
budget.

`--factorization off`, the default in Rust, the CLI, and Python, reduces the
whole graph. `auto` selects the split when the graph has at least two cyclic
blocks and the largest contains at most nine tenths of all cyclic edges.
`force` splits every eligible graph. Dense input uses this path only if routing
first selects the sparse engine. All three modes return the same diagram.

The registered version 0.8 confirmation entry did not show a factorization
speed gain. Automatic factorization took a 4 ms median, while the disabled and
forced arms took 3 ms. Its 0.75 control ratio is a registered preferred-arm
regression. The entry is below a useful timing scale, but it is why
factorization is off by default. This release makes no general speed claim for
the feature.

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

Four schedules are available. The serial schedule sweeps the edges in
passes and removes each qualifying edge as soon as it finds it. It is the
default. The ordered schedule runs the same sweep with worker threads. It
reproduces the serial graph and certificate bit for bit at any worker count.

The rounds schedule tests the live edges against one frozen graph per round.
It removes a batch of independent edges after all tests finish. Its result
does not depend on the worker count, but it can differ from the serial
result.

The adaptive schedule is serial and writes an algorithm version 3
certificate. It scores the removable edges at the start of each pass. The
H1 objective ranks edges by the triangles they remove. The H2 objective
ranks tetrahedra first, then triangles. The schedule tests each planned
removal again against the current graph. A score changes the order, never
the safety rule.

Set `--collapse-work-limit N` to stop the adaptive schedule before it starts
predicate test `N + 1`. The result is a safe partial collapse. Its
certificate says `BudgetLimited` and makes no fixed-point claim. Without a
limit, the final pass proves that the graph is a fixed point. The CLI picks
H1 through `--dim 1` and H2 above it unless you set
`--collapse-objective h1|h2`.

Every schedule records each removed edge, its original value, its schedule
position, and the witnesses that justify it. A version 1 position is a pass.
A version 2 position is a round. A version 3 position is the index in the
removal sequence.

`CollapseArtifact` stores the reduced graph and certificate in a canonical
binary format. SHA-256 digests bind the thresholded input graph and the
reduced graph. The decoder applies byte and collection limits before it
allocates the declared data. A digest binds data to the supplied graph. It
does not authenticate who produced the file.

The independent verifier rebuilds the thresholded input and replays every
removal. It checks each witness at every critical scale. It checks round
independence for version 2, the final graph, the work metadata, and the
fixed-point claim for a complete certificate. It does not reproduce the
adaptive score policy. A passing version 3 artifact proves a safe trace, not
that the producer chose the highest score at each step.

```sh
holos points.csv --collapse-edges                              # serial schedule
holos points.csv --collapse-edges --collapse-schedule ordered --threads 8
holos points.csv --collapse-edges --collapse-schedule adaptive \
  --collapse-objective h1 --collapse-work-limit 100000 \
  --collapse-certificate points.hcol
holos verify-collapse points.csv points.hcol
```

```rust
let params = RipsParams::new(1).with_edge_collapse();
let parallel = RipsParams::new(1)
    .with_threads(8)
    .with_collapse_schedule(holos_tda::CollapseSchedule::Ordered);
let adaptive = RipsParams::new(2).with_adaptive_collapse(
    holos_tda::collapse::AdaptiveCollapseParams::new(
        holos_tda::collapse::CollapseObjective::H2,
    )
    .with_work_limit(100_000),
);
```

```python
bars = holos_tda.rips_points(
    points,
    max_dim=1,
    collapse_edges=True,
    collapse_schedule="adaptive",
    collapse_objective="h1",
    collapse_work_limit=100_000,
)
```

For the certificate itself, use `collapse::collapse_dense` or
`collapse::collapse_sparse`, which take the threshold. The ordered
schedule is `collapse::collapse_dense_ordered_parallel` and
`collapse::collapse_sparse_ordered_parallel`, and the rounds schedule is
`collapse::collapse_dense_rounds_parallel` and
`collapse::collapse_sparse_rounds_parallel`; these take a worker count as
well. The adaptive entry points are `collapse::collapse_dense_adaptive` and
`collapse::collapse_sparse_adaptive`. All entry points return the reduced
matrix, certificate, and run counters. Use
`collapse::verify::verify_dense` or `verify_sparse` for in-process replay.
Use `collapse::wire::CollapseArtifact` for a portable file. The reduced
graph does not depend on the coefficient field or homology dimension, so one
collapse can serve many runs. A collapse artifact alone does not contain a
chain map. The explain profile can lift its own H1 cocycles by replaying the
checked trace in reverse.

Collapse is off by default. Finding removable edges costs time. A smaller
graph does not guarantee a faster complete pipeline. The adaptive schedule
is an opt-in quality and work-budget experiment, not a universal replacement
for the serial schedule.

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

## H1 class spaces, atlases, programs, and graded indexes

The explain profile returns one `PersistentClassSpace` for each distinct
positive H1 interval. Equal intervals form one space with the correct
multiplicity. They do not receive artificial individual identities. The
declared basis is gauge-fixed modulo vertex coboundaries and put in sparse
reduced row-echelon form on the labeled input graph.

`IntervalGroupId` identifies the interval and complete canonical basis.
`BasisClassId` identifies one declared basis vector. The identifiers depend
on vertex labels, the threshold, and the coefficient field. They are not
invariant under relabeling. `LineageId` is separate. It stays fixed while one
atlas is reused, even though endpoint values and content-derived identifiers
change.

A finite cocycle is represented at the largest finite `f64` below its death.
An essential cocycle uses the terminal filtration level. Terms use oriented
edges `(u, v)` with `u < v` and coefficients in Z/p. Each class space also
records the creator edge and optional destroyer triangle for every basis
dimension.

Use `rips_persistence_with_classes` or
`rips_persistence_with_classes_sparse` for a single explained run. A collapse
run lifts each cocycle back to the caller's original graph. The lift verifies
the collapse trace, applies the inverse cochain maps, canonicalizes the whole
class space again, and checks each cocycle on every active triangle. The CLI
writes the same spaces as JSON with `--representatives FILE`.

`PersistenceAtlas` fixes the vertex set, listed edge set, threshold
membership, and complete weak edge-weight order. Under that contract, every
simplex retains its filtration position. The persistence pairing, canonical
class-space basis, and critical simplices stay fixed. `TopologyEvent` states
which part of the contract ended. A fallback update computes a new exact
atlas and starts new lineages.

`ReductionCertificate` proves the H0 and H1 diagram through explicit filtered
boundary matrices and sparse change-of-basis columns. The checker reconstructs
the original boundaries, checks unit-triangular filtration compatibility,
computes each transformed column, and derives the reduced pivots. It does not
call the implicit persistence solver.

`HOLOSRED` stores that algebraic proof. `HOLOSATL` adds the complete input
binding, class spaces, cocycles, critical pairs, and reusable atlas data.
`HOLOSTRC` stores a sequence of graphs and events, with a new `HOLOSATL`
checkpoint only when a region ends. All three version 1 decoders apply
resource limits before allocating their declared collections.

The formats are proof-carrying computation records, not signatures. SHA-256
detects a mismatch with the supplied graph. It does not identify a producer.
The checker proves the recorded algebra and class data. It does not prove
that a producer followed a requested optimization or worker schedule.

`CertifiedReductionRegion` derives a sufficient region from a checked
`D V = R` factorization. Its change-of-basis guards keep `V`
filtration-compatible. Its pivot guards keep `R` reduced. The compiler removes
duplicate and transitively implied comparisons. Unlike an atlas, this region
can accept an edge-order change that does not affect the checked reduction.

`PersistenceProgram` first decomposes a sparse graph into vertex-biconnected
atoms. It then searches each cyclic atom for zero-filtration simplex
separators of width two or three. Such a separator is contractible at every
filtration level, so its H1 barcode composes by direct sum. The bounded search
reports whether it exhausted its candidate set. General nonzero separators
still need persistent interface state and remain WIP.

A changed atom is reused when its guards hold. After a failed guard, repair
reindexes old dependencies by simplex identity. It retains the longest
unit-triangular prefix whose recomputed columns still have distinct pivots,
then reduces the suffix. A topology, threshold-membership, or separator
contract change recompiles the complete program. H0 remains global.

Continuation records use exact equality of canonical cocycle vectors. Exact
correspondence is stronger. It restricts the old and new spaces to their
common filtered subcomplex and computes the intersection of their images over
Z/p. The returned basis states exact linear relations. It does not infer
identity from interval proximity or support overlap.

`HOLOSPRG` binds the decomposition, composed diagram, and nested atom proofs.
`HOLOSDLT` embeds every updated graph and adds a program checkpoint only for
a repaired or recompiled step. `HOLOSINT` adds one restricted finite H1
intervention, its edge edits, checked bounds, and nested update trace. Their
bounded verifiers do not call the persistence solver.

`HOLOSPF` stores unique local `D V = R` reductions once and references them
from every graph snapshot. The `holos-tda-check` crate has no dependency on
`holos-tda`. It reconstructs filtered boundaries, verifies each reduction,
checks the separator decomposition, computes H0, and composes the declared H1
diagram. It caches a weighted reduction when a later snapshot references the
same node with the same local weights. The checker proves the recorded
algebra. It does not authenticate the producer.

`FilteredSimplicialComplex<G>` is the explicit complex boundary used by
relative interfaces. It checks unique cells, face closure, and monotone face
grades before any cancellation. `ScalarGrade` gives the current one-parameter
path a canonical total order. `ProductGrade<N>` represents the coordinatewise
partial order without ordering incomparable grades. A `ScalarProjection`
must be declared before a product-graded complex enters the current scalar
reduction. This API is an exchange contract for other complex builders. It is
not a multiparameter persistence algorithm.

`PersistenceIndex` decomposes the complete listed-edge envelope and maintains
the diagram through `RipsParams::max_dim`. Its bounded search accepts
disconnected splits and arbitrary vertex separators. The default maximum
separator width is four. Separator edges may enter at any filtration value,
and the separator need not be contractible.

The default `InterfacePolicy::Relative` retains one exact filtered chain core
at every node. A child fixes every separator that it shares with any ancestor.
This cumulative protection rule preserves the common chain subcomplex across
the complete recursive composition. A parent identifies matching retained
cells from its children, then cancels equal-filtration pairs outside its own
protected subcomplex. The root has no protected cells and returns the complete
diagram.

`InterfacePolicy::Compose` is a narrower control. It removes a parent
reduction in three checked cases. Disconnected child scopes compose by
multiset union. A connected intersection can be one zero-filtration simplex
or a zero-filtration flag cone. Both forms are contractible at every
filtration value. Other parents retain an exact
`GradedReductionCertificate` over their complete induced graph.

`RelativeInterfaceCertificate` is the general filtered-interface path. It
uses equal-filtration unit cancellations while fixing every cell in the
subcomplex induced by `protected_vertices`. `build_labeled` gives child cores
one global cell namespace. `compose` identifies equal retained cells and
rejects conflicting copies. It does not require the separator to be
contractible, connected, or present at value zero.

`HOLOSRI` version 1 stores the input chain complex, protected vertices,
cancellation trace, retained core, graded `D V = R` reductions, diagram, and
content identifier. The certificate exposes separate source and core
identifiers. The source identifier binds cells that cancellation removes, so
an index update cannot reuse a stale core after such a cell changes.
`holos-check` replays every cancellation and verifies the complete record
without linking to `holos-tda`. The checker also requires the boundary to
square to zero and every protected cell to remain unchanged.

`HOLOSZZ` version 1 stores an affine edge trajectory, fixed scale, field,
dimension, node and arrow claims, all generalized ranks, and interval
multiplicities. The separate checker rebuilds exact rational event times and
flag complexes. It then computes canonical cohomology bases, restriction
maps, generalized ranks, and the complete decomposition without linking to
the producer. Both implementations reject trajectories above the format and
work limits.

`DurableInterfaceStore` writes interface artifacts under SHA-256 ids. Its
`commit_stored` method loads one ordered shard per fold, retains the common
separator, and writes each completed prefix before it starts the next fold.
The final `HOLOSDM` version 1 manifest binds the dimension, field, separator,
output protection, ordered shards, every accumulator, and result. On Unix,
object and record publication flushes files and containing directories.

`holos merge-interfaces` reads and stores each shard before it reads the next
one. Python `merge_relative_interfaces` accepts in-memory artifacts. The
independent `verify_distributed_interface_with` checker requests objects by
content id and retains at most the objects for one fold. `holos-check` accepts
a manifest followed by its referenced object files in any order.

This is a local execution and proof-exchange layer. It does not provide a
network transport, signatures, access control, garbage collection, or
multi-writer coordination. SHA-256 binds ids to bytes but does not
authenticate a producer.

`InterfacePolicy::Materialize` retains a complete graded reduction at every
node and provides a paired control. A bounded search that finds no split keeps
one exact leaf. `IndexSummary` reports the maximum dimension, search
completion, relative input and core cells, cancellations, interface modes,
and largest retained interfaces.

A fixed-envelope transition path-copies each node whose scope contains a
changed edge and keeps every other `Arc` unchanged. A composed parent derives
its diagram from changed children. A touched materialized reduction retains a
valid dependency prefix or rebuilds exactly. An edit can switch a separator
between composed and materialized states. Complete graph updates compare every
listed edge. The current sparse graph storage is also copied, so the release
does not claim sublinear end-to-end work.

`TopologyPatch` groups `set`, `activate`, and `deactivate` edits in one fixed
envelope. `IndexStream` applies patches or complete graph states and emits one
proof record per committed version. A different vertex or listed-edge set
compiles a new tree and emits a cold checkpoint.

`HOLOSIP` version 4 is a complete checkpoint. `HOLOSDP` version 4 stores edge
changes and changed root paths. Each stream declares its maximum dimension.
Each relative node embeds a `HOLOSRI` record. The stateful checker reconstructs
leaf flag complexes from the global graph, replays relative cancellations,
checks cumulative protection, and requires an exact child-core union at every
parent. It does not trust the producer's index objects. Later cold checkpoints
reset its graph state. Old nodes remain available inside one envelope when a
later version returns to a known root. The digest detects content mismatch. It
is not a signature.

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

The adaptive collapse gate crosses both objectives with complete runs and
several work limits on dense and sparse forms. Every result must match the
uncollapsed H1 and H2 diagrams. Every certificate must pass independent
replay. A separate adversarial suite changes certificate metadata, positions,
witnesses, graph bindings, count fields, and envelope lengths, and requires a
specific rejection. Property tests pass arbitrary byte strings to the bounded
artifact decoder and require it to return without a panic.

The point-construction gate compares the k-d tree and exhaustive kernels
against the dense matrix edge by edge. It crosses random and degenerate
clouds, dimensions, finite thresholds, extreme coordinates, and worker
counts. The factorization gate compares forced blocks with whole-graph
reduction on random sparse graphs over several fields and worker counts. An
iterative traversal test also covers a path with 100,000 vertices.

The class gate checks every returned cocycle on all active triangles and
rejects a vertex coboundary. It covers duplicate intervals, canonical
class-space rank, odd fields, worker counts, and every collapse schedule.

The atlas gate compares order-preserving updates with full exact reduction
on randomized sparse graphs over Z/2, Z/3, and Z/5. It exercises every event
kind and checks lineages inside a region. Random point trajectories stay
inside their conservative radius and must match exact threshold-native
reduction. Finite-difference tests check edge and coordinate derivatives.

The independent certificate gate checks randomized graphs across three
fields. Mutations of change-of-basis terms, graph bindings, scalar encodings,
critical pairs, counts, truncations, and trailing bytes must fail. Arbitrary
byte strings pass through each bounded decoder without a panic. A trajectory
gate checks reused steps and independently proved region boundaries without
calling the persistence solver.

The program gate compares result-sensitive evaluation and updates with
monolithic exact reduction on random sparse graphs over Z/2, Z/3, and Z/5.
It covers dependency-frontier repair, zero-filtration separators, atomic
batches, ordered parallel branches, topology recompilation, and exact
correspondence. Artifact tests mutate decompositions, work, events,
continuations, checkpoints, interventions, bounds, and embedded graphs.
Truncation and arbitrary-byte tests cover every bounded decoder. The separate
proof checker also rejects mutated and arbitrary envelopes without linking to
the solver. CLI and Python tests cover proof production and dynamic updates.

The index gate compares every warm version with clean exact compilation over
Z/2, Z/3, and Z/5. Random updates cover H0 through H2. Explicit flag-sphere
fixtures cover H2 and H3. The gate also covers nonzero filtered separators,
disconnected splits, zero-filtration simplex and cone composition, the forced
materialized policy, threshold crossings, active-topology patches, envelope
recompilation, structural sharing, atomic batches, ordered branches, exact
H1 correspondence, and lazy H1 class explanation. The proof gate checks H2
composed and materialized checkpoints, ordered deltas, interface-mode changes,
graded resource limits, repeated roots, mutations, truncation, and
producer-checker agreement.

The relative-interface gate covers a noncontractible H1 separator and H3
composition through a flag 2-sphere. It crosses Z/2, Z/3, and Z/5. A
deterministic sparse-graph sweep compares each core with exact persistence.
Artifact tests cover cancellation replay, mutations, truncations, and
producer-checker agreement.

The relative-index gate compares recursive relative cores with complete
persistence and the materialized policy over Z/2, Z/3, and Z/5. Randomized
updates cover H0 through H2. One adversarial fixture requires descendants to
retain a grandparent separator. Snapshot and delta tests require the separate
checker to reconstruct leaves, replay each core, and reject old proof-format
versions.

The fixed-scale cohomology gate covers H0 through H3 and Z/2, Z/3, and Z/5.
Cross-polytope boundaries pin ranks in H1 through H3. Identity and
filling-edge fixtures pin full and zero relation ranks. Exact event tests
cover simultaneous roots, a nonrepresentable one-third root, persistent
ties, and an H2 class change. Intervention tests cover weighted optimal,
edit-limited infeasible, globally infeasible, and incomplete searches. They
cover equal-cost determinism, different scenario graphs, exact lower bounds,
resource caps, mutations, truncations, and independent replay. A generic
differential test compares the search with flat enumeration on exact
hitting-set predicates.

The kinetic-zigzag gate covers every orientation of a known direct sum over
Z/2, Z/3, and Z/5. Exact trajectory fixtures cover an H2 death, an unchanged
class across an inactive order event, and a simultaneous H1 death and birth
with no class spanning the event. H1 through H3 held-out entries require the
producer and separate checker to agree on every node rank, arrow rank,
generalized rank, and interval multiplicity. Mutation and truncation tests
cover the self-contained artifact.

The synthesis gate compares proof-carrying search with flat enumeration over
canonical subspaces in H1 through H3 and fields Z/2, Z/3, and Z/5. It covers
finite and exact affine sources, state-local and shared actions, complete,
infeasible, and resource-limited results, proof mutations, truncations, and
independent semantic checking.

The relative-coverage gate crosses Z/2, Z/3, and Z/5, failure budgets zero
through two, and coupled and independent state-action components. Flat subset
enumeration, component frontiers, proof-carrying synthesis, and the separate
checker must agree on the exact optimum. It checks every single-byte mutation
and truncation without a panic. CLI and Python gates cover finite and affine
sources. Four SMT obligations check the set-containment, component,
frontier-cost, and boundary-cancellation kernels. They do not verify the Rust
checker or the external controlled-boundary theorem.

## Benchmarks

The engine figures below come from the registered public 0.6 study.
`benchmarks/north_star.sh` measures the shipped binary against ripser and
giotto-ph on a held-out corpus. The corpus carries the decision rule, noise
rule, pinning rule, arms, competitors, timing protocol, and sampling rule.
The study times fresh processes on named physical cores, compares every
diagram before timing, and runs a second copy of the binary as an A/A noise
control.

Version 0.7 adds `benchmarks/collapse_adaptive.sh`. Its frozen screen and
confirmation sets compare no collapse, versions 1 and 2, both version 3
objectives, and resource-bounded version 3 runs. The driver uses a
counterbalanced order. It records each phase on its own clock, counts output
triangles and tetrahedra, verifies every portable artifact, and measures peak
memory in an isolated process per arm. The study does not assume that an
adaptive arm wins. Its decision rule records losses and limits beside wins.

Version 0.8 adds `benchmarks/v08_bench.py`. Its frozen screen and confirmation
sets compare exact point construction with the preserved 0.7 release binary.
They also compare automatic, forced, and disabled graph factorization. Every
diagram must match exactly before the runner keeps a timing.

On the held-out confirmation set, with four workers allowed on four physical
cores and their SMT siblings, exact point construction ran 6.67 times faster
on the low-threshold cube and 2.00 times faster on the mid-threshold sphere.
Peak RSS fell from 26.1 to 4.8 MiB and from 23.3 to 10.3 MiB. These are medians
of five fresh-process runs after one warm-up. The factorization entry took a
4 ms median in automatic mode and 3 ms with factorization disabled or forced.
The 0.75 preferred/control ratio is a registered regression, so factorization
is off by default. The entry is below a useful timing scale, and this release
makes no general factorization speed claim.

Version 0.9 adds `benchmarks/v09_atlas_bench.py`. It compares
`PersistenceAtlas::evaluate_diagram` with full exact H0 and H1 reduction on
the same affine edge-weight trajectory. Graph generation, atlas compilation,
and trajectory construction are outside the update clocks. Each invocation
checks every diagram bit for bit. The registered rule requires at least a
3.0 times update speedup and recovery of compilation within the measured
trajectory.

Both held-out confirmation entries passed. Across 40 updates, atlas reuse
was 4.09 times faster on the 52-point plane entry over Z/3 and 4.75 times
faster on the 48-point volume entry over Z/5. Compilation took 1.103 ms and
1.030 ms. The measured savings recovered it after 30 and 25 updates. These
are serial in-process medians from five counterbalanced repetitions after
one warm-up. The screen failed its rule on one entry because compilation was
not recovered within 25 updates. The study supports repeated evaluation
inside one atlas region. It does not support a general claim about unrelated
inputs or other persistence libraries.

Version 0.10 adds `benchmarks/v10_program_bench.py`. Its accepted trajectory
applies a different additive offset to each complete atom. Each atom keeps
its edge order, while the complete graph crosses its global edge order. Its
repair trajectory crosses one result-sensitive guard and must rebuild exactly
one atom per step. Five counterbalanced repetitions time 120 updates on each
held-out entry. Every diagram must match bit for bit.

The accepted arm passed its registered rule on all three held-out entries.
Program evaluation was 6.06 times faster for 128 five-vertex atoms over Z/3,
6.23 times faster for 160 five-vertex atoms over Z/5, and 5.87 times faster
for 96 six-vertex atoms over Z/2. Compilation was recovered after 59, 60,
and 87 updates. These measurements use one i9-13900KS with the process pinned
to four physical cores and their SMT siblings.

Rich one-atom repair had a 0.97 times held-out median against complete diagram
and canonical-class recomputation. The individual ratios were 0.97, 1.01,
and 0.93. This is inside the preregistered parity band. The study supports a
local reduction-work claim, not a general repair latency win. It also does
not compare version 0.10 with another persistence library.

`benchmarks/v10_application.py` records a deterministic two-atom workflow.
One accepted update splits a rank-two equal-interval space, carries two exact
basis vectors, verifies a self-contained trace, and verifies an optimal
one-edge restricted intervention.

Version 0.11 adds `benchmarks/v11_dynamic_bench.py`. Its state-only arm omits
cross-state correspondence from both sides. It compares dependency-frontier
repair with clean checked program compilation over 48 cumulative updates.
Every diagram and canonical class space must agree exactly.

All three held-out entries passed the registered repair rule. Repair was 2.84
times faster for 48 five-vertex atoms over Z/3, 3.13 times faster for 40
six-vertex atoms over Z/5, and 2.70 times faster for 64 five-vertex atoms over
Z/2. The trajectories retained 666, 1,153, and 696 reduction columns. They
reduced 294, 527, and 264 columns.

Parallel alternatives were 3.95, 4.15, and 5.07 times faster than serial
alternatives at branch counts four, four, and eight. Each entry used five
counterbalanced repetitions after one warm-up. The process was pinned to four
physical cores and their SMT siblings on one i9-13900KS.

The same trajectories produced proof DAGs with 96 unique nodes for 2,352
references, 84 for 1,960, and 112 for 3,136. The independent checker reused
2,256, 1,876, and 3,024 weighted reductions. Its median check times were
5.775 ms, 8.419 ms, and 7.760 ms. Proof construction was outside the clock.
The study does not compare verification with another proof system.

Version 0.12 adds `benchmarks/v12_index_bench.py`. Each constructed graph has
complete weighted atoms joined by one nonzero filtered edge. One update
changes an edge in one atom without changing the envelope or edge order. The
warm arm starts from one compiled index. The cold arm compiles a new index for
every version. Every diagram matches exactly, and the final canonical class
spaces match.

On the three held-out entries, warm updates were 58.83, 120.67, and 122.44
times faster than cold compilation over trajectories of 32, 32, and 40
updates. The graphs contained 24 five-vertex atoms over Z/3, 24 six-vertex
atoms over Z/5, and 32 five-vertex atoms over Z/2. Parallel alternatives were
2.73, 3.28, and 4.40 times faster than serial alternatives at branch counts
four, four, and eight.

Warm proof streams took 0.792, 1.470, and 1.282 MiB. Repeated cold snapshots
took 1.626, 2.956, and 2.685 MiB. Stateful checking was 1.14, 1.16, and 1.04
times faster than checking each cold snapshot. The process used four physical
cores and their SMT siblings on one i9-13900KS. Five counterbalanced timed
runs followed one warm-up. The study covers this shared-edge graph family. It
does not compare another dynamic persistence system or establish a universal
speedup.

Version 0.13 adds `benchmarks/v13_interface_bench.py`. It compares separator
composition with `InterfacePolicy::Materialize` on identical graphs and
updates. Complete weighted atoms meet in one zero-filtration edge. Every
diagram and the final canonical class spaces agree. The paired policy changes
execution structure, not the mathematical input.

On the three held-out entries, composition reduced the largest materialized
scope from 50 to 5 vertices, 98 to 6 vertices, and 98 to 5 vertices. Median
cumulative updates were 10.26, 13.72, and 14.06 times faster than the
materialized policy over 24, 32, and 40 updates. The fields were Z/5, Z/3,
and Z/2.

Complete warm proof streams were 2.73, 3.55, and 2.99 times smaller. Median
stateful check time was 1.141 versus 6.542 ms, 3.541 versus 27.388 ms, and
4.074 versus 27.725 ms. The process used four physical cores and their SMT
siblings on one i9-13900KS. Five counterbalanced timed runs followed one
warm-up. These paired results cover zero-simplex separator graphs. They do not
compare another persistence library or support a universal speed claim.

Version 0.14 adds `benchmarks/v14_graded_bench.py`. It compares graded
separator composition with `InterfacePolicy::Materialize` through H2. Each
graph is a wedge of octahedral flag 2-spheres. Every H0, H1, and H2 diagram
must match at every version. The study also checks the declared execution
structure and both proof streams before it records a timing.

On the three held-out entries, composition reduced the largest materialized
scope from 26 to 6 vertices, 36 to 6 vertices, and 41 to 6 vertices. Median
cumulative updates were 5.80, 7.61, and 8.47 times faster over 10, 12, and 14
updates. The fields were Z/5, Z/3, and Z/2.

Complete warm proof streams were 1.82, 1.94, and 1.97 times smaller. Median
stateful check time was 0.280 versus 1.303 ms, 0.425 versus 2.445 ms, and 0.530
versus 3.420 ms. The process used four physical cores and their SMT siblings
on one i9-13900KS. Five counterbalanced timed runs followed one warm-up. These
paired results cover this constructed H2 family. They do not compare another
dynamic persistence system or support a universal speed claim.

Version 0.15 adds `benchmarks/v15_relative_bench.py`. Each entry composes two
prebuilt filtered chain cores through a common chordless four-cycle. The
separator has one essential H1 class and is not contractible. The runner
checks the composed diagram against complete persistence and checks the
portable artifact before it records descriptive composition, full reduction,
and checker times. The release makes no speed claim from this constructed
family.

On the three held-out entries, the two child cores reduced 96 cells to 56,
144 cells to 80, and 176 cells to 96. Composition took a median 0.089 versus
0.055 ms for complete graded reduction, 0.147 versus 0.097 ms, and 0.261
versus 0.162 ms. It was slower on every entry. Independent checking took
0.034, 0.056, and 0.084 ms. The records came from five repetitions on four
physical cores and their SMT siblings on one i9-13900KS.

Version 0.16 adds `benchmarks/v16_distributed_bench.py`. Each entry folds
ordered relative cores from a fresh content-addressed store, removes the final
manifest, resumes from the durable prefix, and checks every proof object by
id. Every held-out result matched complete persistence and reused all 6, 12,
and 16 folds after the simulated interruption.

On the three held-out entries, shard cancellation reduced 168 cells to 108,
288 cells to 192, and 384 cells to 256. The largest encoded
accumulator-plus-shard counts were 12,818 of 24,978 total shard bytes, 17,790
of 43,188 bytes, and 22,462 of 57,584 bytes. Median clean and recovery times
were 0.897 and 0.276 ms, 2.036 and 0.408 ms, and 3.287 and 0.562 ms.
Independent checking took 0.822, 1.844, and 2.872 ms. Complete graded
reduction took 0.068, 0.125, and 0.181 ms, so both clean composition and
checking were slower on this family. The records came from five repetitions
on four physical cores and their SMT siblings on one i9-13900KS. They do not
support a network, competitor, or general speed claim.

Version 0.17 adds `benchmarks/v17_cohomology_bench.py`. Its held-out entries
cover cross-polytope boundaries in H1, H2, and H3 over Z/5, Z/2, and Z/5.
Canonical fixed-scale rank and full persistence both reported one. Adding one
antipodal edge reduced the fixed-scale rank to zero. The exact affine event
reported the same change. The producer and separate checker both found the
declared one-edge intervention after rejecting one irrelevant candidate.

Median fixed-scale space, relation, event-relation, intervention, checker,
and full-persistence times were 0.002, 0.002, 0.028, 0.014, 0.008, and 0.001
ms in H1. They were 0.006, 0.007, 0.113, 0.041, 0.029, and 0.005 ms in H2,
and 0.030, 0.032, 0.503, 0.197, 0.164, and 0.032 ms in H3. The artifacts used
273, 401, and 593 bytes. The records came from five repetitions on four
physical cores and their SMT siblings on one i9-13900KS. These small
constructed entries validate the algebraic paths. They do not grade a speed
comparison because the fixed-scale and persistence APIs return different
objects.

Version 0.18 adds `benchmarks/v18_relative_index_bench.py`. Each graph has a
chordless four-cycle with 48, 72, or 96 attached triangles. The cycle is a
noncontractible separator. The paired control retains a complete reduction at
every node. Every initial and updated diagram, proof stream, locality check,
and declared compression check must pass before a timing is recorded.

On the three held-out entries, one relative update took 0.257 versus 0.419 ms,
0.416 versus 0.676 ms, and 0.604 versus 0.935 ms. The updates shared 51 of 55,
75 of 79, and 99 of 103 nodes. Relative cancellation retained 558 of 654, 822
of 966, and 1,086 of 1,278 input cells.

Initial compilation times were close: 8.099 versus 8.190 ms, 26.327 versus
26.949 ms, and 64.555 versus 65.648 ms. Relative snapshot and delta pairs used
122.80 and 34.36 KiB, 180.60 and 48.99 KiB, and 238.40 and 63.61 KiB. Full
relative stream checking took 2.545, 3.729, and 4.760 ms. The materialized
control took 0.882, 1.514, and 2.272 ms. Relative proofs were larger and
slower to check because they carry and replay each recursive core.

The records came from five repetitions on four physical cores and their SMT
siblings on one i9-13900KS. This constructed family supports the local update
claim. It does not compare another dynamic persistence system or support a
general speed claim.

Version 0.19 adds `benchmarks/v19_kinetic_zigzag_bench.py`. Its held-out
entries are disjoint unions of flag-sphere boundaries with distinct exact
threshold events. They cover H1 over Z/5, H2 over Z/3, and H3 over Z/5.
Every schedule, node rank, arrow rank, generalized rank, interval
multiplicity, dimension claim, and independent proof must pass before the
script records a timing.

The H1 entry had 12 components, 25 zigzag nodes, and a 7.76 KiB proof.
Complete construction, adjacent-relation calculation, and independent
checking took 15.827, 2.833, and 15.168 ms. The H2 entry had eight
components, 17 nodes, and a 6.23 KiB proof. Its times were 4.859, 3.710, and
4.641 ms. The H3 entry had six components, 13 nodes, and a 6.58 KiB proof.
Its times were 5.363, 5.812, and 5.535 ms.

The records came from five repetitions on four physical cores and their SMT
siblings on one i9-13900KS. These constructed entries validate the complete
algebra and proof paths. They do not grade speed because a complete zigzag and
the adjacent-relations control return different objects.

Version 0.20 adds `benchmarks/v20_intervention_bench.py`. Each entry asks for
one weighted link plan across named classes in disjoint flag-sphere
boundaries. Lower-sorted distractor links join isolated vertices. The exact
flat control shares one base cohomology space and one restriction map per
subset, then checks every subset allowed by the edit limit. Both arms return
the same unique plan. The held-out entries cover H1 over Z/5, H2 over Z/3,
and H3 over Z/5.

In H1, certified search made 46 distinct topology calls while the flat arm
checked 575 subsets. Their median times were 0.811 and 4.458 ms, a 5.50 times
speedup. In H2, the counts were 40 and 377, and the times were 1.597 and
8.173 ms, a 5.12 times speedup. In H3, the counts were 26 and 78, and the
times were 1.731 and 3.137 ms, a 1.81 times speedup. Independent checking
took 0.748, 1.523, and 1.743 ms. The artifacts used 1.27, 2.35, and 2.08 KiB.

The records came from five repetitions pinned to four physical cores and
their SMT siblings on one i9-13900KS. The study uses small constructed
instances and an in-repository exact control. It does not compare another
topology or network-planning system, and it does not support a general speed
claim.

Version 0.21 adds `benchmarks/v21_synthesis_bench.py`. Its held-out entries
constrain canonical cohomology subspaces in H1, H2, and H3 over Z/5, Z/3, and
Z/5. A fourth entry shares actions across three H1 states. The producer, flat
control, and independent checker must agree on each optimum and bound.

On the three independent entries, proof-carrying synthesis made 46, 40, and
26 topology calls. Flat enumeration checked 576, 378, and 79 subsets. Median
producer times were 3.168, 4.789, and 4.346 ms, versus 7.850, 14.392, and
5.671 ms for flat enumeration. Independent checking took 0.325, 0.578, and
0.704 ms. The coupled H1 entry took 0.673 ms for the producer, 0.094 ms for
the checker, and 0.697 ms for flat enumeration. These constructed entries
support exactness and proof-efficiency claims. They do not compare an
external solver or support a general speed claim.

Version 0.22 adds `benchmarks/v22_coverage_bench.py`. It compares exact
failure-tolerant relative coverage synthesis with flat subset enumeration on
fenced-wheel families. Every entry checks the optimum, bounds, proof,
component count, failure cases, and field before it records a timing.

The first held-out independent entry had three states, three incidence
components, failure budget one, and 18 candidates over Z/5. Component
synthesis made 117 topology calls and took 8.666 ms. Flat enumeration checked
31,180 subsets and took 91.433 ms. The second had two components, failure
budget two, and 16 candidates over Z/3. It took 232 calls and 19.698 ms,
versus 14,893 subsets and 42.186 ms. Independent checking took 0.319 and
0.778 ms. The artifacts used 3.13 and 2.47 KiB.

The held-out coupled entry had five states in one component. The producer
took 5.412 ms, while flat enumeration took 0.318 ms. This negative result is
part of the registered record. The measurements use five repetitions on four
physical cores and their SMT siblings on one i9-13900KS. They support the
separated-family result, not a general or competitor speed claim.

On the registered confirmation corpus
(`benchmarks/north_star_confirm_corpus.toml`, 31 entries with seeds disjoint
from every tuning and landing set), on one physical core of an i9-13900KS,
holos took a median 0.37 of the corresponding ripser build's wall time over
the 24 graded headline entries; the largest ratio over all 25 graded entries
was 0.68. An entry whose ripser wall time is under 20 ms is reported but not
graded. By stratum: sparse-selected 0.37 over 25 entries, maxdim-1 0.35 over
20 entries, maxdim-2 0.31 over 3 entries; the dense-selected stratum had one
descriptive entry and no graded entry, because with the routing rule every
dense input of the corpus above the floor routes to the sparse engine.
Public 0.5.0 took a median 1.35 of ripser over the same entries.

At four physical cores without SMT, holos's fresh-process wall time was a
median 0.37 of giotto-ph 0.2.4's in-process `ripser_parallel` time over the
18 graded headline entries; the largest ratio over all 19 graded entries was
0.58. The giotto-ph clock excludes process start and input parsing, which
holos includes, so the comparison favors giotto-ph. On the CPU scaling
entries holos ran 1.9 to 2.0 times faster at four cores than at one.

The matched-precision f64 ripser build is a diagnostic reported in its own
table (29 entries against ripser-f64 and 2 against ripser-coeff-f64), never
pooled with the stock arm. The A/A control gave a noise band of 0.023 on the
serial pass and 0.016 on the multicore pass.

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
