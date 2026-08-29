# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

This entry describes the local 0.7.0 release candidate. It has not been
published.

### Added

- Exact finite collapse portfolios. A portfolio runs serial, rounds, or
  adaptive schedules, verifies every removal, counts surviving flag simplices,
  and selects the exact minimum under an edge or reduction-column objective.
  The `HOLOSPOR` artifact binds every candidate, score, and tie break.
- Proof-carrying persistence for explicit scalar filtered simplicial
  complexes. `ExplicitReductionCertificate` records dimension-generic
  `D V = R` factorizations and exact diagrams in `HOLOSEXP`. The separate
  checker reconstructs faces, boundaries, basis changes, pivots, and bars
  without linking `holos-tda`.
- Geometry-bound finite coverage. `CoverageGeometry` checks a simple
  nondegenerate fence polygon, sensor containment, and the complete Euclidean
  radius graph with exact rational predicates over binary64 input.
  `HOLOSGEO` combines that geometry with a failure-tolerant minimum-cost
  coverage proof. The separate checker validates both layers.
- Canonical H1 class spaces, point and edge sensitivity records, reusable
  persistence atlases, proof-carrying programs, and validity-region events.
- An immutable persistence index with structural sharing, atomic topology
  patches, warm proof deltas, durable streams, and exact diagrams through a
  requested bounded dimension.
- Exact compositional interfaces for disjoint pieces, zero-filtration
  contractible intersections, and arbitrary protected subcomplexes. Local
  content-addressed storage supports ordered folds and recovery. It does
  not provide networking or multi-writer coordination.
- Dimension-generic fixed-scale cohomology spaces, exact relations, affine
  event arrangements, kinetic zigzag intervals, and weighted interventions
  for named classes.
- Exact weighted synthesis over finite and complete affine state schedules.
  Proof trees certify optimal, infeasible, and incomplete results. The
  checker does not rerun branch-and-bound search.
- Relative planar coverage witnesses, maximal-failure reduction,
  state-action component composition, and minimum-cost activation synthesis.
- CLI and Python entry points for the certified workflows. `holos-check`
  recognizes the independent proof, explicit-complex, kinetic, synthesis,
  intervention, relative-interface, distributed-interface, and coverage
  artifacts.
- One consolidated certified-workflow study and one `formal/v07` suite for
  the finite-choice, reduction, coverage, and optimization proof kernels.

### Changed

- The workspace uses Rust edition 2024 and declares Rust 1.85 as its minimum
  supported toolchain.
- Public APIs are organized around filtered complexes, proof artifacts,
  immutable indexes, and typed synthesis specifications. Pre-1.0 source
  compatibility with experimental interfaces is not preserved.
- Historical one-release benchmark drivers were replaced by the v0.7
  certified-workflow study. Published 0.6 engine and collapse studies remain
  unchanged.

### Correctness and limits

- The tracked Rust and Python functions have McCabe complexity below 11.
  Release work rejects any function above that threshold.
- `holos-tda-check` shares mathematical specifications and wire formats with
  the producer, but does not link the producer crate. The collapse portfolio
  still uses the linked collapse verifier.
- The SMT suite checks small logical obligations. It is not a machine proof of
  the Rust implementation or of the controlled-boundary theorem.
- SHA-256 binds artifact content. It does not authenticate a producer.
- Geometry binding accepts finite planar states. Affine coverage remains a
  graph-level conditional claim.
- The new certified workflows make no public performance or priority claim
  until their registered confirmation records exist.

## [0.6.0] - 2026-08-26

### Performance

On the registered confirmation corpus
(`benchmarks/north_star_confirm_corpus.toml`, 31 entries with seeds disjoint
from every tuning and landing set), on one physical core of an i9-13900KS,
holos took a median 0.36 to 0.39 of the corresponding ripser build's wall
time over 24 graded headline entries; the largest ratio over all 25 graded
entries was 0.65 to 0.67. An entry whose ripser wall time is under 20 ms is
reported but not graded. By stratum: sparse-selected 0.36 to 0.38 over 25
entries, maxdim-1 0.35 to 0.41 over 20 entries, maxdim-2 0.31 over 3
entries; the dense-selected stratum had one descriptive entry and no graded
entry, because with the routing rule every dense input of the corpus above
the floor routes to the sparse engine. Public 0.5.0 took a median 1.33 to
1.35 of ripser over the same entries.

At four physical cores without SMT, holos's fresh-process wall time was a
median 0.34 to 0.37 of giotto-ph 0.2.4's in-process `ripser_parallel` time
over 17 to 18 graded headline entries; the largest ratio over all 18 to 19
graded entries was 0.58 to 0.67. The giotto-ph clock excludes process start
and input parsing, which holos includes, so the comparison favors giotto-ph.
On the CPU scaling entries holos ran 1.9 to 2.1 times faster at four cores
than at one.

The matched-precision f64 ripser build is a diagnostic reported in its own
table (29 entries against ripser-f64 and 2 against ripser-coeff-f64), never
pooled with the stock arm. The records report the A/A control for each timed
pass.

The records behind these numbers are attached to the release: the clean
confirmation reproductions (range source), the revealed first run on the
original corpus (superseded), and the engineering landing matrix and tuning
records (evidence, not claims).

### Added

- Engine routing for dense input. `RipsParams::engine`,
  `RipsParams::with_engine`, and `holos --engine auto|dense|sparse` pick the
  engine a dense matrix reduces through. `Engine::Auto` is the default, and
  the Python bindings use it. It converts the matrix to the graph of its
  edges at the resolved threshold when the input has at least 32 points, the
  edge density is at or below four fifths, and the conversion (24 bytes an
  edge and 24 a point) fits a memory budget of 32 MiB or the bytes of the
  compact matrix, whichever is larger.
  `Engine::Dense` keeps the matrix. `Engine::Sparse` converts and ignores
  the budget, because it is an explicit request. An infinite threshold
  routes as well, including the default threshold of a disconnected input:
  such a threshold admits every finite pair and no absent one, so a matrix
  of mostly absent pairs still has a sparse graph. The rule and its
  constants are frozen. The diagram is identical under every setting.
- A storage form for a dense run. `RipsParams::dense_storage`,
  `RipsParams::with_dense_storage`, and `holos --dense-storage
  auto|compact|square` pick the form the dense engine reduces from. holos
  builds every `DistanceMatrix` compact, as the condensed lower triangle.
  `DenseStorage::Auto` is the default and converts to a full row-major
  matrix, which holds both triangles, when a frozen rule reads the compact
  matrix size, a budget on the bytes the second triangle adds, the edge
  count at the resolved threshold, and the distances the cofacet diameter
  fold will read. The full form makes the fold read one contiguous row per
  simplex vertex where the compact form reads a strided column.
  `DenseStorage::Compact` forbids the conversion and `DenseStorage::Square`
  forces it. The choice comes after routing, so a run the routing sends to
  the sparse engine builds no full matrix. The conversion runs once and the
  caller keeps its own matrix, so a run in the full form holds one and a
  half times that form. The diagram is identical under every setting.
- `RipsParams::use_adjacency_rows` and the hidden `--no-adjacency-rows`
  flag, which turn the adjacency rows off for differential tests. They join
  `use_emergent_pairs`, `use_apparent_pairs`, and `use_clearing` as
  optimization toggles that must not change a diagram.
- `io::parse_point_cloud`, `io::parse_condensed`, and `io::parse_triplets`
  parse text a caller already holds, in the grammar the matching reader
  accepts. `io::Triplet` names the `(usize, usize, f64)` a sparse parse
  returns.

### Changed

- `io::read_point_cloud`, `io::read_lower_distance_matrix`, and
  `io::read_sparse_matrix` take a worker budget as a second argument. Pass 1
  for the previous behavior. There is no alias for the old signature.
- `holos --threads` covers the input parse as well as the reduction and the
  collapse. `RipsParams::threads` is unchanged, and the CLI passes it to the
  reader.
- Treat `RipsParams::threads` as the worker budget for a run: the most
  workers it may use, not the number every step must use. The edge sort, the
  dim-0 apparent test, the cofacet assembly, and the column reduction each
  turn their own work estimate into a worker count under that budget, and a
  step with little work runs on one thread. The thresholds are frozen
  constants, and a test pins the decisions they produce: the dim-0 apparent
  test takes a second worker at 128 cycle edges, the assembly at 256 source
  simplices, and the sort at 20,000 elements. The diagram does not change at
  any worker count.
- Store the sparse engine's neighbor lists in one compressed block: one
  offset per vertex, one array of neighbor indices, and one array of
  distances, in place of one vector of (vertex, distance) pairs per vertex.
  The cofacet merge scans the index array and reads a distance only on a
  match, so it touches a quarter of the bytes it did, and a routed dense
  input builds the block directly without a triplet buffer. Every list keeps
  its order, entry for entry.
- Run the dim-0 apparent test on adjacency rows when the graph is dense
  enough for them. The rows hold one bit per pair at or below the threshold,
  with a rank index that reads any edge's distance in constant time. The
  dim-0 walk then sets a second bit set, the activation rows, as it passes
  each edge, so for a cycle edge the largest common bit of its two
  activation rows names the youngest cofacet of equal diameter, and the
  facet check reads at most two distances. The engine builds the rows only
  when the graph has at least 1,024 cycle edges and the adjacency rows,
  their rank index, the activation rows, and the distances together cost at
  most 64 bytes an edge, which is about what the graph itself costs. A
  sparser graph keeps the neighbor-list test. The columns, their order, and
  the diagram do not change at any worker count.
- Read an input file in windows of 16 MiB that end at a newline. A reader
  parses one window at a time, so it holds one window and the parsed values,
  not the whole text, and the first window is sized by the length the file
  reports. With more than one thread, a second thread reads the next window
  while the current one parses. Values, line numbers, and error messages do
  not change.
- Parse input text in line chunks under the worker budget. With more than
  one thread and a text of at least one mebibyte, a window splits into one
  line chunk per worker at newline boundaries, each chunk parses with the
  serial per-line code, and the chunk outputs concatenate in file order.
  Each chunk knows the line number it starts at, so the values are the same
  bit for bit and an error names the same line with the same text as a
  serial parse. A file that holds all its numbers on one line stays serial,
  because a chunk ends at a newline.
- Scan input text by bytes and parse each number in place. The readers
  accept the grammar they accepted before, comma or whitespace separators
  and `#` comment lines, and produce the same values bit for bit. A line
  with a non-ASCII byte takes the previous path.
- Enumerate the sparse engine's cofacets by a descending merge of the
  neighbor lists and report each one as the merge finds it, so a search that
  stops early stops the merge with it. When the caller asks for the upper
  cofacets alone, the merge ends at the highest simplex vertex: no candidate
  at or below that vertex is above every simplex vertex, and the candidates
  descend, so one search of the driver's neighbor list finds where the merge
  stops.
- Stop a cofacet's diameter fold at the first distance above the caller's
  bound. Two callers hold a bound: the apparent-pair test wants a cofacet
  whose diameter equals its simplex's, and the assembly and the coboundary
  keep only what the threshold admits. The dense and the sparse enumerator
  each share one walk between the bounded and the unbounded form, and the
  unbounded form carries no bound test. A sparse source knows its largest
  stored distance, so a threshold at or above that distance raises the bound
  to infinity and leaves a test the walk always passes.
- Take the repeated work out of the apparent-pair tests. The cofacet
  enumerator reports the vertex it adds, so a caller builds the partner's
  vertex set from the set it holds instead of decoding the partner's index
  again. One classifier answers both directions of the test and writes only
  into buffers its caller owns, so a test allocates nothing. The
  zero-apparent back-check keeps a simplex's pairwise distances in one
  table, adds the distance from the added vertex to each simplex vertex, and
  reads every facet diameter as a maximum over that table; each facet index
  comes from the vertices by the combinadic identity `idx_below - C(v_k,
  k+1) + idx_above`, so the walk runs no binary search. With the added
  vertex above every simplex vertex the back-check answers without reading
  anything. The facet order, the tie rules, and the diameter comparisons are
  unchanged, so the apparent pairs are the same.
- Decode an edge index without the general search. `BinomialTable::unrank`
  takes a shortcut at dimension 1, through the new
  `BinomialTable::unrank_edge`. The upper vertex is the largest `v` with
  C(`v`, 2) at or below the index, which one integer square root gives
  exactly, and the lower vertex is what the index has left. The reduction
  decodes an edge for every dim-1 column and for every edge of the dim-0
  pass, so the two binomial searches this replaces sat in its innermost
  loops. The vertices are the same at every index.
- Order the reduction's sorts and heaps by packed integer keys. A diameter
  here is a maximum of validated distances, finite and not negative, so its
  bits order as a `u64` exactly as `f64::total_cmp` orders the value. The
  dim-0 edge sort now runs on `u128` keys that carry the diameter bits above
  the complemented index, and the walk decodes each key back to its edge. A
  working column's heap entry is the single 128-bit key its order sorts by,
  the complement of the diameter bits over the packed index and coefficient,
  so ordering the heap costs one integer comparison. Both keys are
  bijections, and the threshold test carries `+inf` as `f64::MAX`, so one
  comparison decides membership. The orders and the pops are the ones the
  two-field comparators gave.
- Order a working column with one heapify. The reduction collects the
  column's cofacets first, then orders them all at once, where it sifted
  each cofacet into the heap as it arrived. The cofacets go into the vector
  the heap already owns, so a column allocates nothing the previous column
  did not. The pivots, the pops, and their coefficients do not change.
- Multiply a cofacet's boundary sign into its coefficient without dividing.
  The sign is 1 or p - 1, so `Coeffs::mul_sign` returns the coefficient or p
  minus the coefficient, and `Fp::neg` subtracts instead of taking a
  remainder. `Fp::mul` still divides for the general product, and the
  coefficients are unchanged.
- Store the binomial table k-major, the layout ripser uses. The cofacet and
  facet enumerators hold `k` fixed and step the vertex down by one, so
  consecutive lookups are now neighbors in memory and the stride no longer
  grows with the highest dimension.
- Reserve the serial reducer's pivot map for the columns of the dimension,
  and sort the serial assembly's columns with an unstable sort. The keys are
  (diameter, index) pairs, unique inside a dimension, so a stable sort buys
  nothing and its scratch buffer costs an allocation.
- Pair an emergent pivot on the serial path without testing it again. The
  coboundary scan takes the emergent shortcut only after it proves that no
  column holds the pivot and that the pivot has no zero-apparent facet. The
  serial reducer owns its pivot map, so it now records the pair at once. A
  parallel worker shares its map with the other workers and still repeats
  both tests.
- Classify dim-0 apparent cofacets in parallel. The union-find walk stays
  serial and keeps the cycle edges it finds. Workers then take blocks of
  those edges and test each one against the frozen distances, into one
  result slot per edge. On the activation rows the worker count also picks
  the shape of the walk: one worker takes one diameter group at a time, and
  more than one takes a block of 8,192 sorted edges, decodes the block on
  the pool, and tests the block's cycle edges on the pool. The columns
  follow in walk order, so the diagram and the column order do not depend on
  the worker count.
- Take the busiest shared writes off the parallel reducer's per-column path.
  A worker counts the columns it finished and subtracts them from the shared
  outstanding count only when the queue runs dry, and the queue's three
  counters sit on separate cache lines. A worker that finds the queue dry
  while a column is still in flight yields a few times, then sleeps for one
  microsecond, doubling up to 128 microseconds, instead of reading the
  shared counters in a tight loop. A displaced column still finds a worker
  within that time. Column priority, the claim rule, and the pivot table are
  unchanged, so the pivot registry is the same at every worker count.
- Count the degrees before building a sparse matrix.
  `SparseDistanceMatrix::from_triplets` walks the triplets once to size
  every neighbor list, so no list grows by reallocation. The lists it builds
  are the same, entry for entry.
- Fold the enclosing radius one row at a time. The row's own maximum stays
  in a local until the row ends, and only the column maxima go back to
  memory. `DistanceMatrix::enclosing_radius` returns the same value.
- Write the diagram through a 64 KiB buffer. Standard output flushes at
  every line, so a diagram of fifty thousand bars took fifty thousand
  writes. It now takes a few.

## [0.5.0] - 2026-08-17

### Added

- Two parallel edge collapse schedules. Both write certificates the
  independent verifier accepts, both give the same diagram as the serial
  collapse, and both are deterministic at every worker count.
  - The ordered schedule, `collapse::collapse_dense_ordered_parallel` and
    `collapse::collapse_sparse_ordered_parallel`, runs the serial schedule
    on worker threads. Workers test a bounded window of the due edges
    against one immutable state of the graph. The window then retires in
    serial order: an edge reuses its cached test when no removal committed
    since the test can have changed the result, and otherwise the test
    runs again against the current graph. The reduced graph and the
    certificate are the serial ones at every worker count, field for
    field, with each floating value compared by bits. The certificates are
    algorithm version 1. A gate covers dense and sparse input, worker
    counts 0, 1, 2, 4, and 8, and window sizes from one edge to larger
    than the whole input, and it compares against the serial collapse and
    against an independent unpruned reference.
  - The rounds schedule, `collapse::collapse_dense_rounds_parallel` and
    `collapse::collapse_sparse_rounds_parallel`, removes edges in rounds.
    Each round tests the live edges against a frozen snapshot of the graph
    and deletes a batch whose removals provably do not affect each other,
    so the batch is equivalent to removing those edges one at a time in
    the recorded order. The reduced graph and the certificate are
    identical at every worker count, field for field, with each floating
    value compared by bits. They are not the serial ones: the two
    schedules can keep different edges. A gate enforces the identity at 1,
    2, 4, and 8 threads, and the diagram equality battery runs against the
    serial collapse, the uncollapsed run, and the oracle.
- `CollapseSchedule` on `RipsParams`, with
  `RipsParams::with_collapse_schedule` and the CLI flag
  `--collapse-schedule serial|ordered|rounds`. It picks the schedule the
  pipeline runs when `collapse_edges` is set. The default is the serial
  schedule. The Python bindings expose `collapse_edges` alone and run the
  serial schedule; the `holos-tda` console script carries the flag.
- Algorithm version 2 certificates for the rounds schedule.
  `CollapseCertificate::algorithm_version` reports 2, and each removal
  step records its round. `RemovalStep::epoch` returns a step's epoch, and
  `CollapseStats::epochs` returns the epoch count. An epoch is a pass for
  version 1 and a round for version 2. A step does not carry the version,
  so read the version from the certificate.
- Round checks in the independent verifier. For a version 2 certificate it
  rebuilds the graph before each round and validates every witness of that
  round against that one snapshot, with the checks it already applied to
  version 1. It then checks that no removal of the round has both
  endpoints in the closed common neighborhood of another removal of the
  same round, that the recorded order within the round holds, and that the
  round numbers are contiguous. The verifier rejects a round that groups
  removals which affect each other, even when the same removals in
  sequence would pass. The independence check examines the endpoint pairs
  inside each closed common neighborhood instead of every pair of
  removals, so a wide round of small neighborhoods costs the sum of the
  squared neighborhood sizes, not the round width squared. Version 1
  certificates keep their existing checks.
- Scheduling counters on `CollapseStats` for the ordered schedule.
  `logical_tests` is the test count of the serial schedule.
  `invalidated_results` counts cached tests dropped before use, whether a
  conflicting removal or a large-neighborhood bail invalidated them; every
  dropped test runs again serially at its turn, so this is also the repair
  count. `global_invalidations` counts the large-neighborhood bails that
  dropped at least one cached test ahead of them, and `window_batches`
  counts the window stages executed. `window_slots_offered` and
  `window_members_formed` give window occupancy, and
  `window_members_reused` counts the cached verdicts consumed without a
  repair. `edge_tests` keeps its meaning of physical predicate calls,
  which speculation can push above the logical count. The structural
  fields are identical at every worker count and window size; the
  scheduling counters, `edge_tests`, and `max_common_neighborhood` are the
  fields that move.
- `CollapseTimings` on `CollapsedRips` splits an ordered run's wall clock
  into the parallel test phase, the serial retirement walk, and the repairs
  inside it. It is a diagnostic: the values vary between runs, never
  affect an output field, and are zero on the serial and rounds schedules.
- A phase-separated scaling benchmark (`crates/collapse-bench`,
  `benchmarks/collapse_scaling_rounds.sh`,
  `benchmarks/collapse_scaling_ordered.sh`). It times the distance build,
  the graph construction, the whole collapse call, and the downstream
  reduction in one process, each on its own clock, so it measures collapse
  time directly instead of subtracting it from a total. It asserts diagram
  equality across every configuration before it reports a timing.

### Changed

- With `RipsParams::collapse_edges` set, `RipsParams::threads` is the
  worker budget for the whole pipeline. One pool serves the reduction and,
  when a parallel schedule runs, the collapse. A standalone parallel
  collapse call owns a pool for its duration.
- The CLI reports passes for the serial and ordered schedules and rounds
  for the rounds schedule.
- `CollapsedRips` is `#[non_exhaustive]` and carries a `timings` field;
  construct it from a collapse call only. `CollapseStats` implements
  `Default`. `RemovalStep::pass` and the `passes` field are gone. Use
  `RemovalStep::epoch` and `CollapseStats::epochs`.
- Performance, from two preregistered scaling studies (held-out
  confirmation sets, four physical cores with two threads each). Neither
  parallel schedule beats the serial collapse end to end on the median
  entry, so the serial collapse stays the default and the throughput
  recommendation in the confirmed regimes.
  - The ordered schedule, graded negative under its frozen rule. At four
    workers the median confirmed entry runs the collapse at 0.84x the
    serial speed and the whole pipeline at 0.85x. The serial collapse is
    faster end to end on ten of the thirteen confirmed entries; the
    ordered schedule wins three, by at most 1.13x. Peak memory is a median
    1.06x and at most 1.15x of the serial pipeline. The cost is repair.
    Earlier removals invalidate most cached tests, which then run again
    serially, and on the median entry those repairs alone take about three
    quarters of the whole serial collapse phase. The windows still stay at
    or above 99 percent full on every headline entry, and the physical
    test count stays inside its 2x bound. The ordered schedule gives a
    parallel run whose reduced graph and certificate are exactly the
    serial ones, bit for bit.
  - The rounds schedule, graded parallel-but-Amdahl-limited under its
    frozen rule. The collapse itself scales with workers, a median 4.8x
    from one worker to eight, and that does not carry to the pipeline. The
    complete run is slower than the same pipeline with the serial collapse
    on every confirmed entry, because the rounds schedule tests many more
    edges to reach its fixed point, and the gap grows with the edge count.
    Peak memory is a median 1.35x and at most 1.43x of the serial
    pipeline. The rounds schedule gives a result that does not depend on
    the worker count and, on some inputs, a much smaller reduced graph.
    One confirmation entry keeps 2,199 edges where the serial schedule
    keeps 16,815.
  - The point estimates above come from the registered runs. Their
    records, the exact corpus files they cite, and a reproduction at a
    release candidate commit are attached to the release. The registered
    runs cite internal development commits that are not in the public
    history; the reproduction, whose timings and peak memory are
    descriptive, gave the same grades and the same numbers within
    rounding.

## [0.4.0] - 2026-08-12

### Added

- Filtered edge collapse. Set `--collapse-edges` on the CLI,
  `RipsParams::collapse_edges` (or `with_edge_collapse()`) in the Rust
  library, or `collapse_edges=` in Python. The collapse removes edges whose
  absence cannot change any bar, and the engine then runs on the smaller
  graph. The diagram is unchanged in every dimension: the test battery
  requires bar-for-bar equality against the uncollapsed run across a range
  of moduli, thresholds, and thread counts, and every optimization-toggle
  combination. Collapse is off by default.
- Standalone collapse API: `collapse::collapse_dense` and
  `collapse::collapse_sparse` return the reduced `SparseDistanceMatrix`, a
  `CollapseCertificate`, and run counters. The reduction does not depend on
  the coefficient field, the homology dimension, the optimization toggles,
  or the thread count, so one collapsed graph serves many runs.
- `CollapseCertificate`: a replayable record of every removal, with the
  edge, its value, the pass, and the witness data that certifies it. The
  certificate and the reduced graph reconstruct the thresholded input.
- An independent certificate verifier, `collapse::verify::verify_dense` and
  `verify_sparse`. It rebuilds the graph, re-derives the criterion from the
  specification, checks every recorded witness directly at every scale
  where the edge's neighborhood changes, and confirms that no further edge
  is removable. It shares no code with the collapse itself.
- `SparseDistanceMatrix::edges()`: every stored edge once, as endpoints and
  value.
- A preregistered break-even study for the collapse
  (`benchmarks/collapse_bench.sh`, `benchmarks/collapse_corpus.toml`). The
  families, the parameter grid, the sampling rule, and the decision rule
  were written down before the measurements. See `benchmarks/README.md`.

### Changed

- With collapse enabled, dense input runs through the sparse enumerator,
  because the reduced graph is sparse. The diagram is identical either way.
- The collapse resolves the threshold before it runs, by the same rule the
  engine uses. Surviving edge values are the input values, bit for bit.

## [0.3.1] - 2026-08-05

Packaging and release-infrastructure patch. No engine or API changes.

### Changed

- The repository is a cargo workspace: `crates/holos-tda` (the core crate)
  and `crates/holos-tda-py` (the Python wrapper). The wrapper depends on
  the core crate by path, so wheels and sdists build without waiting for
  a crates.io publish. Each crate has its own directory, which removes
  the file collision behind the 0.2.1 sdist failure.
- One tag runs the whole release: version gate, a preflight that packages
  the crate and re-tests it extracted at the MSRV, wheels and sdist,
  crates.io, PyPI. The GitHub release with installers still comes from
  the same tag.
- The crate archive no longer bundles the benchmark scripts; the README
  points to the repository for them.
- Benchmark scripts truncate their result tables on rerun instead of
  appending stale blocks.

## [0.3.0] - 2026-08-04

### Added

- Parallel reduction. Set `--threads N` on the CLI, `RipsParams::threads`
  in the Rust library, or `threads=` in Python. Workers reduce the columns
  of each dimension concurrently and out of order; pivot ownership follows
  the column order. The diagram is identical at any thread count. A
  determinism test enforces bar-for-bar equality.
- Benchmark scripts for parallel scaling, for sparse input, and for an
  equal-core comparison against giotto-ph (`benchmarks/`).

### Changed

- Sparse input enumerates cofacets through neighbor-list intersection. A
  sparse filtration now saves enumeration time as well as memory.
- The serial engine is faster. Coefficients pack into the entry word. The
  top dimension is not materialized. The enclosing radius takes one pass
  over the distances. The reducer allocates less.

## [0.2.1] - 2026-07-25

### Fixed

- Packaging: the Python source distribution failed to build because the
  wrapper crate vendored the root crate through a path dependency, colliding
  on `README.md` and the license files. The wrapper now depends on the
  published `holos-tda` crate, so the sdist builds and wheels publish to
  PyPI. No library, CLI, or Rust API changes.

## [0.2.0] - 2026-07-25

### Added

- Coefficients in a prime field Z/p, p < 32768: `--modulus` on the CLI,
  `RipsParams::modulus` (and `with_modulus`) in the library. Z/2 stays the
  default and keeps its exact v0.1 code path and performance. Validated by
  a mod-p oracle, differential tests against a coefficient-enabled ripser
  build, and a projective-plane fixture whose H1/H2 differ between Z/2 and
  Z/3.
- Sparse distance input: `SparseDistanceMatrix`, `rips_persistence_sparse`,
  and `--format sparse` (ripser-compatible `i j d` triplets). Pairs not
  listed are absent at every scale; distance storage is O(n + edges)
  instead of the dense O(n^2).
- Python bindings, published as `holos-tda` on PyPI: `import holos_tda` for
  the library (`rips_points`, `rips_condensed`, `rips_sparse`), and a
  `holos-tda` console script that is the same CLI (works under `uvx`).
  abi3 wheels for Linux, macOS, and Windows.
- The CLI is callable as a library function (`holos_tda::cli::run_cli`).

## [0.1.0] - 2026-07-24

First public release.

### Added

- Exact Vietoris-Rips persistent homology barcodes over Z/2 for point clouds
  and precomputed lower-distance matrices, with a ripser-class implicit
  persistent cohomology engine (clearing, emergent pairs, apparent pairs,
  union-find for H0, enclosing-radius default threshold).
- Library API: `rips_persistence`, `DistanceMatrix`, `RipsParams`, `Diagram`,
  `Bar`, plus build-provenance constants (`VERSION`, `GIT_HASH`,
  `BUILD_PROFILE`).
- CLI binary `holos` with ripser-compatible input handling, ripser-style and
  CSV output, and debug toggles for each solver optimization.
- Independent brute-force oracle and release-gating validation: exhaustive
  small-space sweeps, property tests, and differential testing against a
  pinned ripser build. H0/H1/H2 are certified.
- Reproducible benchmark harness (`benchmarks/run.sh`) that refuses dirty
  trees, records full provenance, and fails on any diagram mismatch.

[Unreleased]: https://github.com/t0rsion/holos/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/t0rsion/holos/releases/tag/v0.6.0
[0.5.0]: https://github.com/t0rsion/holos/releases/tag/v0.5.0
[0.4.0]: https://github.com/t0rsion/holos/releases/tag/v0.4.0
[0.3.1]: https://github.com/t0rsion/holos/releases/tag/v0.3.1
[0.3.0]: https://github.com/t0rsion/holos/releases/tag/v0.3.0
[0.2.1]: https://github.com/t0rsion/holos/releases/tag/v0.2.1
[0.2.0]: https://github.com/t0rsion/holos/releases/tag/v0.2.0
[0.1.0]: https://github.com/t0rsion/holos/releases/tag/v0.1.0
