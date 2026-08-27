# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.22.0] - 2026-08-26

### Performance

The registered held-out study compared proof-carrying coverage synthesis with
exact flat subset enumeration. Both independent entries used separate
state-action components. The Z/5 entry had three states, failure budget one,
and 18 candidates. Component synthesis made 117 topology calls and took a
median 8.666 ms. Flat enumeration checked 31,180 subsets and took 91.433 ms.
The Z/3 entry had two states, failure budget two, and 16 candidates. The
corresponding values were 232 calls and 19.698 ms, versus 14,893 subsets and
42.186 ms.

Independent checking took 0.319 and 0.778 ms on those entries. Their proof
trees required 39 and 27 topology checks. The artifacts used 3.13 and 2.47
KiB.

The held-out coupled entry did not benefit from component frontiers. The
producer took 5.412 ms, while flat enumeration took 0.318 ms. The study used
constructed fenced-wheel families, an in-repository exact control, five
repetitions, and four physical cores with their SMT siblings on one
i9-13900KS. It supports the separated-family result, not a general or
competitor speed claim.

### Added

- An exact controlled-boundary relative coverage criterion. A canonical
  fence cycle must bound a two-chain in the active Rips complex over the
  selected prime field. The result includes the filling chain.
- An explicit planar radius contract. It checks the exact dyadic inequality
  `3 * sensing_radius^2 >= broadcast_radius^2` and keeps the external domain,
  placement, fence, and communication assumptions visible.
- Failure-tolerant weighted sensor activation across finite communication
  states. The evaluator checks every maximal failure set, which is sufficient
  by activation monotonicity.
- Exact compilation of affine communication-edge weights at one broadcast
  radius. Endpoints, threshold events, and one rational sample from every
  open time cell cover the complete closed interval.
- Exact state-action incidence components, local cost-cardinality frontiers,
  and dynamic-program composition under one global activation limit.
- `HOLOSCOV` version 1. The bounded artifact stores the physical contract,
  finite or affine source, states, actions, failures, optimum or checked gap,
  evaluations, proof tree, work counts, and SHA-256 digest.
- Independent `HOLOSCOV` verification in `holos-tda-check`. The checker has
  its own decoder, affine compiler, finite-field boundary solver, failure
  enumeration, and proof interpreter.
- `holos cover` and `holos cover-affine`, plus Python `relative_coverage`,
  `synthesize_coverage`, and `synthesize_affine_coverage` functions.
- Machine-checked SMT obligations for maximal-failure containment, component
  feasibility, frontier cost composition, and boundary cancellation.
- Exhaustive flat-enumeration, unequal-cost proof-order, mutation,
  truncation, CLI, and Python gates for the coverage path.

### Changed

- Proof branches now use canonical action-index order. Unequal action costs
  previously exposed an internal mismatch between proof construction and the
  proof decoders.
- The Rust crate, checker crate, and Python package are version 0.22.0.

### Limits

- The physical coverage conclusion is conditional on the controlled-boundary
  domain, sensor-placement, fence, and communication assumptions. The
  artifact does not contain coordinates or prove those facts.
- The criterion is sufficient, not necessary. Rejection does not prove a
  physical coverage hole.
- Affine inputs describe exact communication-edge weights. The release does
  not prove that they arise from Euclidean sensor trajectories.
- Actions activate non-fence sensors with positive integer costs. Fences do
  not fail. The release does not optimize placement, motion, sensing radii,
  edge edits, or probabilistic failure.
- Search, failure enumeration, triangle enumeration, and proof construction
  can be exponential. Work limits return a checked gap when available.
- The checker is ordinary safe Rust. The SMT files cover four proof kernels,
  not the finite-field implementation, the recursive checker, or the external
  controlled-boundary theorem.

## [0.21.0] - 2026-08-26

### Added

- Canonical cohomology subspaces in any supported dimension and prime field.
  Reduced coordinates identify the subspace, not one chosen generating family.
- Finite topological specifications. A state bounds the dimension of the
  intersection between a target subspace and an edited restriction image.
  One weighted action set must satisfy every state under a global edit limit.
- State-specific action support and exact state-action incidence components.
  State evaluation discards irrelevant actions before using its exact cache.
- A complete proof tree for optimal and infeasible synthesis results. Its
  rules use surviving maximal sets, edit and cost leaves, disjoint necessary
  sets, and an exhaustive first-selected-member branch partition.
- `HOLOSSYN` version 1. The bounded artifact stores the specification,
  actions, selected plan, cost bounds, proof tree, rank claims, source scope,
  work limits, and SHA-256 content digest.
- Independent `HOLOSSYN` verification in `holos-tda-check`. The checker has
  its own flag-complex enumeration, finite-field cohomology, restriction
  maps, subspace operations, decoder, and proof rules. It checks the proof
  tree instead of repeating branch-and-bound.
- Exact compilation of an affine edge trajectory into a complete fixed-scale
  specification. The source-bound artifact includes both endpoints, each
  threshold event, and one exact rational sample from every open time cell.
  The checker reconstructs this schedule before accepting the all-time claim.
- `holos synthesize` for listed sparse states and `holos
  synthesize-kinetic` for affine trajectories. Both write artifacts accepted
  by `holos-check`.
- Python `synthesize_cohomology` and `synthesize_affine_cohomology` functions.
  They return the artifact, status, plan, bounds, producer work, proof work,
  and state ranks.
- Machine-checked SMT obligations for the necessary-set, disjoint-bound, and
  branch-partition rules. A registered H1 through H3 study compares producer
  search, proof checking, and exact flat subset search.

### Changed

- `KineticFiltration::critical_graphs` now emits only fixed-scale threshold
  events. Edge-order events cannot change the active graph, so they no longer
  add redundant synthesis states or pairwise event work.
- The Rust crate, checker crate, and Python package are version 0.21.0.

### Limits

- Synthesis adds declared edges with positive integer costs. It does not
  remove edges, move vertices, choose a changing scale, or infer candidate
  actions.
- One artifact uses one vertex set, cohomology dimension, fixed scale, and
  prime field. A listed-state source makes no claim outside those states. An
  affine source covers its complete declared closed time interval.
- Search and proof construction remain exponential in the worst case.
  Producer limits return a checked incumbent and lower bound when available.
  Proof limits can reject a complete result whose certificate is too large.
- Incidence components identify independent state predicates. The current
  solver keeps one global search because the edit limit can couple component
  cost and cardinality frontiers.
- An all-time Rips cohomology rank condition is a graph-topology condition.
  It does not by itself certify physical sensor coverage. An exact geometric
  or relative coverage criterion remains WIP.
- The checker is ordinary safe Rust. The SMT files cover three proof rules,
  not the finite-field implementation or the complete recursive checker.
  SHA-256 binds bytes but does not authenticate a producer.

## [0.20.0] - 2026-08-26

### Performance

The registered held-out study compared certified search with exact flat
subset search on weighted link-planning fixtures. It covered H1 over Z/5, H2
over Z/3, and H3 over Z/5. Both arms returned the same unique minimum-cost
plan in every entry, and the separate checker accepted every artifact.

In H1, certified search made 46 distinct topology calls while the flat arm
checked 575 subsets. Median times were 0.811 and 4.458 ms, a 5.50 times
speedup. In H2, the corresponding counts were 40 and 377. Times were 1.597
and 8.173 ms, a 5.12 times speedup. In H3, the counts were 26 and 78. Times
were 1.731 and 3.137 ms, a 1.81 times speedup.

Independent checking took 0.748, 1.523, and 1.743 ms. The self-contained
artifacts used 1.27, 2.35, and 2.08 KiB. The study used five repetitions
pinned to four physical cores and their SMT siblings on one i9-13900KS. These
constructed entries compare with one in-repository exact control. They do not
support a competitor or general speed claim.

### Added

- Exact weighted branch-and-bound search for an antitone survival predicate.
  Exact subset caching avoids repeated topology calls.
- Necessary candidate sets. Disjoint sets give additive cost and cardinality
  lower bounds. The search branches on a set that every feasible plan must
  hit.
- Shared interventions across up to 256 declared graph scenarios. One
  selected edge set must kill every named canonical cohomology class.
- Positive integer candidate costs, edit limits, oracle and node limits,
  exact infeasibility, and checked lower and upper bounds after an incomplete
  search.
- `CohomologyRestriction::image_contains`, which tests membership in an exact
  restriction image over the declared prime field.
- `HOLOSCI` version 2. The separate checker reconstructs each flag complex,
  cohomology space, and restriction map. It repeats the weighted search and
  compares its status, plan, bounds, work counts, ranks, and root necessary
  sets.
- `holos plan-links`, which reads several sparse scenario graphs and writes
  one independently checked minimum-cost link plan.
- A registered H1 through H3 study with frozen screen and held-out corpora.
  Its flat control checks every candidate subset under the edit limit.

### Changed

- `CohomologyInterventionArtifact::build` now accepts scenarios, weighted
  candidates, an edit limit, and separate oracle and node limits.
- `holos intervene-cohomology` now requires `--candidate U V COST` and
  `--max-edits`. It reports cost bounds and distinct topology calls.
- Python `intervene_cohomology` now accepts `(graph, target)` scenarios and
  `(u, v, cost)` candidates. It returns cost bounds, search work, root
  necessary sets, and per-scenario ranks.
- The Rust crate, checker crate, and Python package are version 0.20.0. The
  API and `HOLOSCI` changes are breaking pre-1.0 changes.

### Limits

- Optimality covers only the declared candidate edges and positive integer
  costs. The result is subject to `max_edits`.
- Every scenario shares one vertex set, dimension, scale, field, and candidate
  set. The release does not model probabilities, capacities, uncertain
  costs, edge removal, or a changing scale.
- The search remains exponential in the worst case. Each topology call uses
  explicit flag-complex enumeration and sparse field reduction. Work limits
  return a checked gap instead of an optimality claim.
- A killed cohomology class is an exact topological condition. It does not by
  itself prove a physical network performance or reliability property.
- The checker repeats the semantic search, so its work can approach producer
  work. It is ordinary Rust code, not a formally verified proof. SHA-256
  binds bytes but does not authenticate a producer.

## [0.19.0] - 2026-08-25

### Performance

The registered held-out study used disjoint flag-sphere boundaries with exact,
distinct threshold events. It covered H1 over Z/5, H2 over Z/3, and H3 over
Z/5. Every exact schedule, dimension claim, adjacent relation, complete
zigzag decomposition, artifact, and independent check passed.

For 12 H1 components, the trajectory had 25 nodes and a 7.76 KiB proof.
Building the complete zigzag took 15.827 ms. Computing only adjacent
relations took 2.833 ms, and independent checking took 15.168 ms. For eight
H2 components, the corresponding results were 17 nodes, 6.23 KiB, 4.859 ms,
3.710 ms, and 4.641 ms. For six H3 components, they were 13 nodes, 6.58 KiB,
5.363 ms, 5.812 ms, and 5.535 ms.

The study used five repetitions on four physical cores and their SMT siblings
on one i9-13900KS. These constructed entries validate the global algebra and
proof paths. They do not grade speed because the complete zigzag and the
adjacent-relations control return different objects.

### Added

- Exact cohomology restriction matrices in canonical quotient bases.
  `cohomology_restriction` checks the labeled subcomplex inclusion and returns
  the induced map in the declared field.
- `ZigzagModule`, arbitrary adjacent arrow directions, generalized ranks on
  every contiguous submodule, and exact interval decomposition over a prime
  field.
- Interval-isotypic identifiers. Repeated equal summands have one content
  identifier and an exact multiplicity. The API does not invent identities
  for indistinguishable copies.
- `KineticFiltration::cohomology_zigzag`. Open time cells alternate with exact
  event complexes, so simultaneous births and deaths cannot be matched by
  proximity or iteration order.
- `HOLOSZZ` version 1. The separate checker reconstructs the rational event
  schedule, every flag complex, canonical cohomology space, restriction map,
  generalized rank, and interval multiplicity.
- `holos kinetic --zigzag FILE`, automatic `holos-check` dispatch, and Python
  `kinetic_zigzag` records with the artifact, nodes, arrows, generalized-rank
  matrix, and intervals.
- A registered kinetic-zigzag study with frozen screen and held-out corpora.
  It covers H1 through H3, three fields, complete event schedules, proof
  replay, and a paired adjacent-relation control.

### Changed

- The Rust crate, checker crate, and Python package are version 0.19.0.
- Kinetic class output can now describe the complete fixed-scale trajectory.
  The earlier adjacent-event relation API remains available.

### Limits

- The result is fixed-scale cohomology over one kinetic time parameter. It is
  not a two-parameter module or a vineyard over the full scale filtration.
- Generalized-rank decomposition is an explicit exact algorithm. Its work can
  grow cubically in the node count before sparse linear-algebra costs. Both
  the producer and checker enforce a 2,049-node format cap and a charged work
  cap of 100,000,000.
- An interval with multiplicity greater than one is a canonical isotypic
  space, but its individual equal summands are not canonical.
- The checker is ordinary Rust code, not a formally verified proof. SHA-256
  binds artifact bytes but does not authenticate a producer.

## [0.18.0] - 2026-08-25

### Performance

The registered held-out study used chordless four-cycle separators with 48,
72, and 96 attached triangles. Each separator has one essential H1 class.
Every initial diagram, updated diagram, proof stream, locality check, and
declared compression check passed.

One relative-core update took 0.257 versus 0.419 ms, 0.416 versus 0.676 ms,
and 0.604 versus 0.935 ms for the materialized control. The updates shared 51
of 55, 75 of 79, and 99 of 103 nodes. Cancellation retained 558 of 654, 822
of 966, and 1,086 of 1,278 input cells.

Initial compilation took 8.099 versus 8.190 ms, 26.327 versus 26.949 ms, and
64.555 versus 65.648 ms. Relative snapshots used 122.80, 180.60, and 238.40
KiB. Their deltas used 34.36, 48.99, and 63.61 KiB. Full relative stream
checking took 2.545, 3.729, and 4.760 ms. The materialized control took 0.882,
1.514, and 2.272 ms. Relative proofs were larger and slower to check because
they carry and replay each recursive core.

The study used five repetitions on four physical cores and their SMT siblings
on one i9-13900KS. These results cover one constructed family. They do not
compare another dynamic persistence system or support a general speed claim.

### Added

- `FiltrationGrade`, `LinearFiltrationGrade`, `ScalarGrade`, and
  `ProductGrade<N>`. Product grades use the coordinatewise partial order and
  do not invent an order between incomparable values.
- `FilteredSimplex<G>` and `FilteredSimplicialComplex<G>`. Construction checks
  unique cells, face closure, vertex labels, cell order, and monotone grades.
  `ScalarProjection` and `CoordinateProjection` give an explicit boundary
  from product grades to the current scalar persistence engine.
- `InterfacePolicy::Relative`, the new index default. Each node retains an
  exact filtered core relative to every separator shared with an ancestor.
  Updates rebuild one affected route and share untouched `Arc` subtrees.
- Source identifiers for relative certificates. A source identifier binds
  the complete pre-cancellation complex. The existing core identifier binds
  the retained semantic core.
- `HOLOSIP` and `HOLOSDP` version 4. Each relative node embeds its `HOLOSRI`
  record. The separate checker reconstructs leaf flag complexes from the
  global graph, replays cancellations, checks cumulative protection, and
  requires an exact child-core union at each parent.
- Relative input, core, cancellation, and transition work fields in
  `IndexSummary`, `IndexWork`, `InterfaceSummary`, proof summaries, and Python
  records.
- A registered recursive relative-index study with frozen screen and held-out
  corpora. It compares one local update with the materialized policy through
  a noncontractible H1 separator.

### Changed

- `IndexParams::default()` and `holos index` now select relative cores.
  `--materialize-interfaces` retains a complete reduction at every node.
- Python `compile_sparse_index` replaces the old composition Boolean with
  `interface_policy="relative"`. It also accepts `"compose"` and
  `"materialize"`.
- Internal index composition trusts already checked, owned child cores. The
  public `RelativeInterfaceCertificate::compose` entry point still verifies
  each supplied certificate.
- The Rust crate, checker crate, and Python package are version 0.18.0. The
  proof-format and Python argument changes are breaking pre-1.0 changes.

### Limits

- Product grades are a checked representation and projection boundary. This
  release does not compute multiparameter persistence modules or invariants.
- The explicit flag-complex path can grow exponentially with dimension and
  graph density. Sparse reduction can fill in. Resource limits reject work
  before declared bounds are exceeded.
- Equal-filtration cancellation produces an exact core, not a smallest core.
  The result depends on the documented deterministic cell order.
- Relative updates still copy the sparse graph. The release does not claim
  sublinear end-to-end space or time.
- Version 4 proofs carry complete recursive core records. They were larger
  and slower to check than materialized proofs on the registered family.
- The independent checker is not formally verified. SHA-256 binds content
  under its collision-resistance assumption but does not authenticate it.
- The registered study uses one constructed family on one CPU. It does not
  support a competitor, large-instance, or general speed claim.

## [0.17.0] - 2026-08-25

### Performance

The registered held-out study used cross-polytope boundaries in H1, H2, and
H3 over Z/5, Z/2, and Z/5. Canonical fixed-scale cohomology and full
persistence both reported rank one. Adding one antipodal edge reduced the
fixed-scale rank to zero. Every exact affine crossing reported the same rank
change, and the separate checker accepted every optimal intervention.

Median space and relation times were 0.002 and 0.002 ms in H1, 0.006 and
0.007 ms in H2, and 0.030 and 0.032 ms in H3. Median event-relation,
intervention, and checker times were 0.028, 0.014, and 0.008 ms in H1; 0.113,
0.041, and 0.029 ms in H2; and 0.503, 0.197, and 0.164 ms in H3. The
self-contained artifacts used 273, 401, and 593 bytes.

The study used five repetitions on four physical cores and their SMT siblings
on one i9-13900KS. These descriptive results cover small constructed
complexes. Fixed-scale bases and full persistence return different objects,
so the study does not grade a speed comparison.

### Added

- `cohomology_space`, a deterministic basis of fixed-scale `H^q` over a prime
  field. It enumerates the active flag complex through dimension `q + 1` and
  computes `kernel(delta_q) / image(delta_(q-1))` under explicit resource
  limits.
- `cohomology_relation`, which restricts two spaces to their common active
  subcomplex and returns an exact basis of the image intersection with source
  coefficients on both canonical bases.
- `KineticFiltration`, exact rational event schedules for affine edge
  trajectories, adjacent-float root enclosures, persistent-tie counts, and
  fixed-scale class relations on adjacent open event cells.
- `CohomologyInterventionArtifact` and `HOLOSCI` version 1. The search checks
  declared absent-edge candidates in cardinality order and reports
  `Optimal`, `Infeasible`, or `SearchIncomplete` with checked bounds.
- Independent `HOLOSCI` replay in `holos-tda-check`. The checker reconstructs
  every flag complex, canonical cohomology basis, restriction image, and
  candidate subset without depending on the producer crate.
- `holos cohomology`, `holos kinetic`, and `holos intervene-cohomology`.
  Python adds `cohomology_space`, `cohomology_relation`, `affine_events`, and
  `intervene_cohomology`.
- Tests for H0 through H3, Z/2, Z/3, and Z/5, identity and filling relations,
  simultaneous and nonrepresentable event roots, search bounds, mutations,
  truncations, CLI output, Python output, and producer-checker agreement.
- A registered exact-class study with frozen screen and held-out corpora. It
  grades ranks, event changes, finite-candidate optimality, independent
  replay, and artifact size.

### Changed

- `CohomologyInterventionLimits::with_max_subsets` sets a search cap without
  constructing the non-exhaustive limits type.
- The Rust crate, checker crate, and Python package are version 0.17.0. These
  additions establish new pre-1.0 API and artifact contracts.

### Limits

- These are fixed-scale cohomology classes. They are not identities for
  persistence intervals, homology cycles, or classes across unrelated vertex
  sets.
- Flag-complex enumeration can grow exponentially with dimension and graph
  density. Sparse row reduction can also fill in. Resource limits reject
  inputs rather than make the computation implicit.
- Kinetic weights must be affine. Input floats are interpreted as exact
  dyadic rationals. Events at the two interval endpoints are boundary state,
  not interior events.
- Intervention optimality covers only the finite candidate set supplied by
  the caller. Complete search is exponential in its size. An incomplete
  search reports no feasible upper bound.
- The independent checker is not formally verified. SHA-256 binds content
  under its collision-resistance assumption but does not authenticate it.
- The registered study uses small constructed complexes on one CPU. It does
  not support a competitor, large-instance, or general speed claim.

## [0.16.0] - 2026-08-25

### Performance

The registered held-out study folded 6, 12, and 16 ordered relative-interface
shards through a noncontractible four-cycle. Every result matched complete
exact persistence. After the final manifest was removed, retries reused every
durable fold.

Shard cancellation reduced 168 cells to 108, 288 cells to 192, and 384 cells
to 256. The largest encoded accumulator-plus-shard counts were 12,818 of
24,978 total shard bytes, 17,790 of 43,188 bytes, and 22,462 of 57,584 bytes.

Median clean and recovery times were 0.897 and 0.276 ms, 2.036 and 0.408 ms,
and 3.287 and 0.562 ms. Independent checking took 0.822, 1.844, and 2.872 ms.
Complete graded reduction took 0.068, 0.125, and 0.181 ms. Clean composition
and checking were slower than complete reduction on every entry.

The study used five repetitions on four physical cores and their SMT siblings
on one i9-13900KS. These descriptive results cover one constructed family.
They do not support a network, competitor, or general speed claim.

### Added

- `DurableInterfaceStore`, immutable SHA-256 objects, ordered streaming folds,
  durable prefix records, atomic result publication, and exact work counters.
  `commit_stored` loads one shard per fold instead of retaining all shards.
- `HOLOSDM` version 1 manifests. A manifest binds the dimension, field,
  separator, output protection, ordered shard ids, every fold id, and the
  final result id.
- Independent manifest replay in `holos-tda-check`. The callback API reloads
  objects by id and retains at most one fold's artifacts. The checker verifies
  every shard, keyed child-core union, parent cancellation, reduction, and
  final protection change without linking to the producer.
- `holos merge-interfaces`, distributed-manifest support in `holos-check`, and
  Python `merge_relative_interfaces`.
- Recovery tests from complete and partial durable prefixes, corrupted-object
  tests, incompatible-shard tests, CLI coverage, and producer-checker
  agreement.
- A registered durable-composition study with frozen screen and held-out
  corpora. It grades exactness, prefix reuse, the encoded-buffer bound, and
  relative-core reduction.

### Changed

- Object, progress, and manifest publication flushes file contents. On Unix,
  it also synchronizes containing directories after rename.
- `ProofError::new` is public so an external proof-object source can report a
  bounded loading failure.
- The Rust crate, checker crate, and Python package are version 0.16.0. These
  additions intentionally establish new pre-1.0 artifact and API contracts.

### Limits

- The store is local. It has no network transport, remote scheduler,
  signatures, authentication, garbage collection, or multi-writer locking.
- SHA-256 content ids detect mismatch under the collision-resistance
  assumption. They are not signatures.
- The encoded `peak_artifact_bytes` counter is not process RSS. Decoded sparse
  maps, the keyed union, and allocator overhead require additional memory.
- The independent checker is not formally verified. It proves submitted
  algebra, not worker scheduling or artifact origin.
- Canonical class spaces, class relations, kinetic events, and interventions
  remain H1-only or WIP outside this execution layer.
- The registered study uses a constructed family on one CPU. Clean
  composition and checking were slower than complete reduction on it.

## [0.15.0] - 2026-08-25

### Performance

The registered held-out study joined two filtered complexes through a
noncontractible four-cycle. The child cores reduced 96 cells to 56, 144 cells
to 80, and 176 cells to 96. They used 20, 32, and 40 equal-filtration
cancellations. Every composed diagram matched complete exact persistence, and
the separate checker accepted every artifact.

Median composition time was 0.089 versus 0.055 ms for a complete graded
reduction, 0.147 versus 0.097 ms, and 0.261 versus 0.162 ms. Relative
composition was slower on all three entries. Median independent check time was
0.034, 0.056, and 0.084 ms. The artifacts used 7,175, 10,703, and 13,051
bytes.

The study used five repetitions on four physical cores and their SMT siblings
on one i9-13900KS. These descriptive results cover one constructed family.
They do not support a general speed claim.

### Added

- `RelativeInterfaceCertificate`, an exact filtered chain core relative to a
  protected separator subcomplex. It records labeled cells, equal-filtration
  unit cancellations, the retained core, graded `D V = R` reductions, and the
  derived diagram.
- Exact `RelativeInterfaceCertificate::compose`. Child cores identify cells
  by global vertex labels. Composition supports arbitrary filtered
  separators, including disconnected and noncontractible intersections with
  nonzero persistent homology.
- `HOLOSRI` version 1 and independent checking in `holos-tda-check`. The
  checker verifies the chain condition, replays every cancellation, requires
  protected cells to remain fixed, verifies each reduction, and derives the
  diagram without the persistence solver.
- `holos interface` and Python `compile_relative_interface`. Both paths expose
  protected vertex labels, maximum dimension, coefficient field, exact work,
  and portable proof bytes.
- Tests for noncontractible H1 separators, H3 composition through a flag
  2-sphere, Z/2, Z/3, and Z/5, deterministic graph sweeps, byte mutations,
  truncations, CLI output, and producer-checker agreement.
- A registered relative-interface study with frozen screen and held-out
  corpora. Each entry composes through a chordless four-cycle and checks the
  complete artifact independently.

### Changed

- The workspace uses a dedicated `interface-bench` binary for paired
  relative composition and complete graded-reduction measurements.
- The Rust crate, checker crate, and Python package are version 0.15.0. These
  additions intentionally establish new pre-1.0 artifact and API contracts.

### Limits

- Relative cancellation is deterministic but does not produce a minimal
  core. A core can be as large as its input, and explicit flag enumeration can
  grow exponentially.
- The immutable index proof stream still uses contractible composition or a
  complete graded fallback. It does not yet carry recursive `HOLOSRI` cores.
- The independent checker is not formally verified. The artifact is not a
  signature, an authenticated record, or a succinct cryptographic proof.
- Higher-dimensional barcodes are exact, but canonical class spaces,
  correspondence, and interventions remain H1-only.
- The registered study uses a constructed separator family on one CPU. Its
  timings do not support a universal speed claim or a competitor claim.

## [0.14.0] - 2026-08-24

### Performance

The registered held-out study compared graded separator composition with the
same index forced to retain every parent reduction. Each graph was a wedge of
octahedral flag 2-spheres. Both policies processed identical versions through
H2. Every H0, H1, and H2 diagram matched at every version.

Composition reduced the largest materialized scope from 26 to 6 vertices, 36
to 6 vertices, and 41 to 6 vertices. Median cumulative update time was 5.80,
7.61, and 8.47 times faster than the materialized policy. The trajectories
contained 10, 12, and 14 updates over Z/5, Z/3, and Z/2.

Complete warm proof streams were 1.82, 1.94, and 1.97 times smaller. Median
stateful check time was 0.280 versus 1.303 ms, 0.425 versus 2.445 ms, and 0.530
versus 3.420 ms.

The study used five counterbalanced repetitions after one warm-up. It ran on
four physical cores and their SMT siblings on one i9-13900KS. Generation,
initial compilation, proof construction, and correctness checks were outside
the clocks.

### Added

- `GradedReductionCertificate`, a dimension-generic `D V = R` proof for an
  explicit filtered flag complex. It records one boundary reduction for every
  simplex dimension from one through `max_dim + 1`.
- `GradedReductionRepair` and per-dimension work records. A transition repairs
  each graded boundary independently and derives one exact diagram.
- Exact diagram composition in every maintained dimension for disconnected
  covers and covers whose common intersection is a zero-filtration
  contractible flag complex.
- `InterfaceMode::ZeroCone`. The index recognizes a nonclique separator when
  all active separator edges enter at zero and one separator vertex is joined
  at zero to every other separator vertex.
- Dimension-generic immutable indexes in Rust, the `holos index --dim` option,
  and the Python `compile_sparse_index(..., max_dim=...)` argument.
- Independent graded proof checking. The checker reconstructs each boundary,
  verifies `D V = R`, derives every requested homology dimension, and checks
  composed parents without calling the persistence solver.
- Differential tests through H3, randomized index updates through H2, graded
  resource-limit tests, and producer-checker tests for H2 snapshots and warm
  deltas.
- A registered H2 separator study with frozen screen and held-out corpora. It
  checks execution structure, exact diagrams, proof size, update time, and
  checker time.

### Changed

- `PersistenceIndex` now maintains the complete diagram through
  `RipsParams::max_dim`. The previous H0 and H1 limit is removed.
- Materialized index nodes now retain `GradedReductionCertificate` instead of
  the H0 and H1 `ReductionCertificate`.
- `HOLOSIP` and `HOLOSDP` are version 3. Each stream declares `max_dim`, and
  each materialized node carries graded boundary columns. Version 2 is
  rejected.
- Interface content identifiers use version 3 and bind the maximum dimension
  and every graded reduction.
- Index summaries and Python records report the maximum dimension and one
  boundary-column count per simplex dimension. These are intentional pre-1.0
  API breaks.

### Limits

- Canonical class spaces, correspondence, and interventions still cover H1.
  A graded index maintains exact higher-dimensional bars but does not assign
  higher-dimensional class identities across versions.
- The graded certificate enumerates the explicit flag complex through
  `max_dim + 1`. Clique counts can grow exponentially with the vertex count.
  Resource limits reject oversized artifacts before their declared
  collections are allocated.
- Composition covers disjoint children and zero-filtration intersections
  certified as a simplex or a cone. Other separators use a complete
  materialized reduction. This is not a general persistent Mayer-Vietoris
  implementation.
- The held-out results cover constructed wedges of flag 2-spheres and one CPU.
  They compare two policies in this implementation. They do not compare
  another dynamic persistence system or establish a universal speedup.
- Complete graph updates still copy the current sparse graph and compare every
  listed edge. This release makes no sublinear end-to-end worst-case claim.
- The independent checker is a separate implementation, but it is not
  formally verified, authenticated, or cryptographically succinct.

## [0.13.0] - 2026-08-24

### Performance

The registered held-out study compared certified separator composition with
the same index forced to retain every parent reduction. Each graph contained
complete weighted atoms joined by one zero-filtration edge. Both policies
processed identical versions. Every diagram matched at every version, and
the final canonical H1 class spaces matched.

Composition reduced the largest materialized scope from 50 to 5 vertices,
98 to 6 vertices, and 98 to 5 vertices. Median cumulative update time was
10.26, 13.72, and 14.06 times faster than the materialized policy. The
trajectories contained 24, 32, and 40 updates over Z/5, Z/3, and Z/2.

Complete warm proof streams were 2.73, 3.55, and 2.99 times smaller. Median
stateful check time was 1.141 versus 6.542 ms, 3.541 versus 27.388 ms, and
4.074 versus 27.725 ms.

The study used five counterbalanced repetitions after one warm-up. It ran on
four physical cores and their SMT siblings on one i9-13900KS. Generation,
initial compilation, proof construction, and correctness checks were outside
the clocks.

### Added

- Exact composed interfaces for disconnected covers and zero-filtration
  simplex separators. A composed parent retains its diagram, child digests,
  separator labels, and routing scope. It retains no boundary reduction over
  the parent scope.
- `InterfaceMode`, `InterfacePolicy`, and new index summary fields. The
  default `Compose` policy uses a checked composition rule where it applies.
  `Materialize` provides an exact paired baseline and explicit fallback.
- Exact H0 and H1 composition for a cover whose child intersections are one
  zero-filtration simplex. H1 is the direct sum of child modules. H0 removes
  one essential interval for each duplicate copy of the intersection.
- `TopologyPatch` and `IndexEdit::Activate`. An atomic patch can set,
  activate, or deactivate listed edges while preserving the fixed envelope.
- `IndexStream`, `IndexStreamStep`, and `IndexStreamProof`. A stream emits a
  warm delta after a fixed-envelope step and a cold checkpoint after an
  envelope change. Ordered patch batches commit atomically.
- Mixed-record checking. `holos-check` accepts later cold checkpoints in the
  same ordered stream and resets its verified graph state before continuing.
- Python active-topology patches, interface modes, composition summaries, and
  an explicit materialized-interface policy.
- Differential tests for composed and materialized policies over Z/2, Z/3,
  and Z/5. Proof tests exercise composed cold snapshots, composed warm
  deltas, policy changes, and active-topology transactions.
- A registered paired separator-interface study with frozen screen and
  held-out sets. It checks execution structure, exact results, proof size,
  update time, and checker time.

### Changed

- `HOLOSIP` and `HOLOSDP` are version 2. Every node declares its interface
  mode and exact diagram. A materialized node carries `D V = R` columns. A
  composed node carries no reduction columns, and the checker derives its
  diagram from already checked children. Version 1 is rejected.
- `PersistenceIndex::diagram` now reads the root interface. The root can be
  composed and need not contain a reduction over the complete graph.
- A transition recomputes its algebraic summary because an edit can switch a
  separator between composed and materialized states.
- `IndexUpdateMode::Composed` reports a changed route that required no parent
  reduction. `IndexWork::nodes_composed` counts those interfaces.
- `holos index` pairs each `--update` with `--record`. A record is a warm
  `HOLOSDP` delta or a replacement `HOLOSIP` checkpoint. The old `--delta`
  option is removed.
- `IndexParams` now includes `interface_policy`. Python summary and interface
  records include composition fields. These are intentional pre-1.0 API
  breaks.

### Limits

- Composition covers disjoint children and children whose common separator
  is a simplex at filtration value zero. A general filtered separator uses a
  complete materialized reduction. This release does not implement a general
  persistent Mayer-Vietoris interface.
- Indexes and index proofs cover H0 and H1. The ordinary persistence engine
  remains dimension-generic.
- A fixed-envelope patch updates only the affected algebraic route, but the
  current graph value storage is copied. A complete graph update also
  compares every listed edge. This release makes no sublinear end-to-end
  worst-case claim.
- Adding or removing a listed envelope edge recompiles the separator tree and
  emits a cold checkpoint. Activation and deactivation are local only when
  the edge was listed when the index was compiled.
- The held-out speed results cover constructed zero-simplex separator graphs
  and one CPU. They compare two policies in this implementation. They do not
  compare another dynamic persistence system or establish a universal
  speedup.
- The independent checker verifies the composition rule and every retained
  reduction. It is not formally verified, authenticated, or
  cryptographically succinct.

## [0.12.0] - 2026-08-24

### Performance

The registered held-out study compared warm index transitions with cold index
compilation. Each constructed graph contained complete weighted atoms joined
by one nonzero filtered edge. Updates changed one atom without changing the
listed-edge envelope or edge order. Every diagram matched exactly at every
version. Final canonical H1 class spaces also matched.

Warm updates were 58.83 times faster for 24 five-vertex atoms over Z/3,
120.67 times faster for 24 six-vertex atoms over Z/5, and 122.44 times faster
for 32 five-vertex atoms over Z/2. The trajectories contained 32, 32, and 40
updates. They shared 736, 736, and 1,240 tree nodes across those updates.

Parallel alternatives were 2.73, 3.28, and 4.40 times faster than serial
alternatives at branch counts four, four, and eight. Warm proof streams were
2.05, 2.01, and 2.10 times smaller than repeated cold snapshots. Stateful
checking was 1.14, 1.16, and 1.04 times faster than checking each snapshot
separately.

The study used five counterbalanced repetitions after one warm-up. The
process ran on four physical cores and their SMT siblings on one i9-13900KS.
Proof construction was outside the verification clocks.

### Added

- `PersistenceIndex`, an immutable exact H0 and H1 index over a sparse
  listed-edge envelope. Its deterministic bounded search accepts disconnected
  splits and arbitrary vertex separators. Separator edges may have any
  filtration value and need not form a contractible filtered complex.
- Exact materialized interfaces for every separator-tree node. Each node
  carries a checked `D V = R` reduction for its induced graph. The complete
  root reduction remains the authority for the global diagram.
- Path-copying fixed-envelope transitions. Touched nodes retain a valid
  dependency prefix or rebuild. Untouched `Arc` subtrees remain physically
  shared between immutable versions.
- `IndexTransition`, `IndexWork`, `IndexEvent`, `DiagramDelta`, `IndexDiff`,
  and `IndexEdit`. Transitions report threshold crossings, repaired or rebuilt
  reductions, envelope recompilation, exact work, and diagram changes.
- Optional exact class correspondence through `transition_with`. The ordinary
  `transition` keeps class work off the update path. `explain` computes
  canonical H1 class spaces on demand.
- Atomic ordered batches, parallel ordered branches, immutable forks, version
  content identifiers, interface inspection, and physical-sharing counts.
- Canonical, bounded `HOLOSIP` version 1 cold snapshots and `HOLOSDP` version 1
  warm deltas. A delta stores changed edge values and changed root paths.
- Stateful index checking in the separate `holos-tda-check` crate. It checks
  induced scopes, child covers, separator intersections, every `D V = R`,
  content digests, and the exact root diagram. It applies each delta atomically
  and retains known nodes for later roots.
- `holos index INPUT SNAPSHOT`, with paired `--update` and `--delta` options.
  `holos-check SNAPSHOT [DELTA ...]` verifies the ordered stream. The checker
  still accepts one `HOLOSPF` trajectory proof.
- Python `SparseIndex` and `compile_sparse_index`. The object exposes cold and
  warm proof bytes, updates, atomic batches, forks, diffs, interfaces, exact
  work, optional correspondence, and lazy explanations.
- Differential tests over Z/2, Z/3, and Z/5. New tests cover nonzero filtered
  separators, disconnected graphs, threshold crossings, envelope changes,
  sharing, batch rollback, branch order, repeated proof roots, mutations, and
  truncation.
- A registered versioned-index study with frozen screen and held-out sets. Its
  generated records separate update, branch, proof size, and checker clocks.

### Changed

- All workspace crates now use the Rust 2024 edition. The minimum supported
  Rust version remains 1.85.
- The recommended repeated-update interface is now `PersistenceIndex`.
  `PersistenceAtlas` and `PersistenceProgram` remain available for their
  narrower region and intervention contracts.
- `holos-check` now keeps verified index state across an ordered delta stream.
  A return to an earlier digest is accepted only when the carried node content
  matches the retained content exactly.

### Limits

- Indexes cover H0 and H1. The ordinary persistence engine remains
  dimension-generic.
- A general separator can materialize its complete induced reduction. The
  root always contains the complete reduction. This release does not claim a
  small persistent Mayer-Vietoris interface or sublinear worst-case updates.
- Separator search is deterministic and bounded. Failure to find a separator
  keeps one exact leaf. `IndexSummary` reports the search count and whether it
  completed.
- Warm deltas require one modulus, threshold, vertex set, and listed-edge
  envelope. An envelope change needs a new cold snapshot.
- Exact class correspondence is separate from state maintenance and can be
  quadratic in the number of class spaces.
- The held-out speed results cover constructed shared-edge graphs on one CPU.
  They do not cover unrelated topologies, other machines, or another dynamic
  persistence system.
- Index proofs are deterministic and solver-independent, not formally
  verified, authenticated, or cryptographically succinct.

## [0.11.0] - 2026-08-24

### Performance

The registered held-out study compared state-only dependency-frontier repair
with clean checked program compilation. Both arms omitted cross-state class
correspondence. Every diagram and canonical class space matched exactly over
48 cumulative updates.

Repair was 2.84 times faster for 48 five-vertex atoms over Z/3, 3.13 times
faster for 40 six-vertex atoms over Z/5, and 2.70 times faster for 64
five-vertex atoms over Z/2. The trajectories retained 666, 1,153, and 696
reduction columns. They reduced 294, 527, and 264 columns.

Parallel alternatives were 3.95, 4.15, and 5.07 times faster than serial
alternatives at branch counts four, four, and eight. The study used five
counterbalanced repetitions after one warm-up. The process ran on four
physical cores and their SMT siblings on one i9-13900KS.

The proof DAGs stored 96 unique nodes for 2,352 references, 84 for 1,960, and
112 for 3,136. The independent checker reused 2,256, 1,876, and 3,024
weighted reductions. Its median check times were 5.775 ms, 8.419 ms, and
7.760 ms. Proof construction was outside the clock.

### Added

- Dependency-frontier reduction repair. A repair reindexes old
  change-of-basis dependencies by simplex identity. It retains the longest
  unit-triangular prefix whose recomputed columns have distinct pivots, then
  reduces only the suffix.
- Exact `ClassCorrespondence` records across updates. Each record restricts
  old and new class spaces to their common filtered subcomplex and computes
  the intersection of their images over Z/p. It returns a canonical basis for
  the exact linear relation.
- Exact H1 composition across zero-filtration simplex separators of width two
  or three. The deterministic bounded search reports its checked candidate
  count and whether it completed.
- `ProgramCheckpoint`, atomic `advance_batch`, deterministic `branch`, and
  their explicit correspondence-mode variants. Parallel branches preserve
  input order and do not change their source program.
- `CorrespondenceMode`. `Exact` remains the default. `Omit` maintains the
  exact current diagram, class spaces, proofs, events, and work while leaving
  cross-state correspondence empty.
- Canonical, bounded `HOLOSPF` version 1 proof DAGs. Unique local reductions
  are content-addressed and referenced across complete graph trajectories.
- The separate `holos-tda-check` crate and `holos-check` binary. The checker
  has no dependency on `holos-tda`. It reconstructs boundaries, checks every
  `D V = R` reduction and separator, computes H0, and composes H1.
- `holos prove INPUT OUTPUT [UPDATE ...]`. It writes one `HOLOSPF` trajectory
  for point, dense-distance, or sparse inputs.
- Python state-only updates, atomic `update_many`, ordered `fork`, the current
  `SparseProgram.proof`, and `compile_sparse_proof`.
- Random dependency-repair tests across Z/2, Z/3, and Z/5. New gates cover
  exact correspondence rank, wider separators, batch rollback, parallel
  branch order, proof mutations, truncation, and producer-checker agreement.
- A registered self-adjusting study with frozen screen and held-out sets. Its
  generated records separate repair, branching, proof structure, and checker
  time.

### Changed

- Accepted result-sensitive updates now reindex and check their reductions
  against the new filtration. A program artifact captured after reuse is
  therefore bound to the current graph.
- `HOLOSDLT` is version 2. It records dependency-repair work and bounded exact
  class correspondence. Version 1 traces are rejected without an alias.
- Program summaries now report zero-filtration separator count, largest
  separator width, candidate count, and search completion.
- Program repair work distinguishes retained and reduced columns and records
  column additions. Python work and summary tuples contain the new fields.

### Limits

- Programs and unified proofs cover H0 and H1. The ordinary persistence
  engine remains dimension-generic.
- Wider composition covers only zero-filtration clique separators of width at
  most three. Nonzero separators require persistent interface state. Search
  stops safely at 100,000 candidates and reports an incomplete result.
- Exact correspondence can be quadratic in the number of class spaces.
  `CorrespondenceMode::Omit` skips it and makes no cross-state relation claim.
- Repair uses one retained prefix per boundary dimension. It does not update
  a disconnected set of later columns around an invalid middle column.
- Parallel branches clone the checked source state. They are not a shared
  mutable reduction forest.
- `HOLOSPF` can be larger than a clean implicit recomputation. The checker is
  deterministic and solver-independent, not formally verified or
  cryptographically succinct.
- Digests bind proof content. They do not authenticate a producer or certify
  that a scheduling policy was followed.

## [0.10.0] - 2026-08-24

### Performance

On the registered held-out many-atom set, accepted program evaluation was
6.06 times faster than exact diagram recomputation for 128 five-vertex atoms
over Z/3. It was 6.23 times faster for 160 five-vertex atoms over Z/5 and
5.87 times faster for 96 six-vertex atoms over Z/2. Each arm evaluated 120
updates. The clocks were counterbalanced across five repetitions after one
warm-up. Every diagram matched bit for bit.

Compilation took 10.605 ms, 13.920 ms, and 16.504 ms. The measured savings
recovered it after 59, 60, and 87 updates. Every held-out entry passed the
registered 2.5 times and amortization rule.

The separate one-atom repair arm compared rich program updates with complete
diagram and canonical-class recomputation. Its held-out median was 0.97
times, inside the registered 0.95 through 1.05 parity band. The individual
ratios were 0.97, 1.01, and 0.93. This release makes no general repair speed
claim.

### Added

- `CertifiedReductionRegion`, with sufficient filtration guards derived from
  a checked `D V = R` reduction. Change-of-basis guards preserve the unit
  triangular basis change. Pivot guards preserve reduced pivots. The compiler
  removes duplicate and transitively implied comparisons and evaluates each
  simplex formula once.
- `PersistenceProgram`, which composes exact positive H1 persistence over
  vertex-biconnected atoms and computes H0 globally. `evaluate_diagram`
  performs checked reduction-free evaluation. `advance` reuses valid atoms,
  rebuilds invalid atoms locally, or recompiles after a topology or threshold
  event.
- `ProgramWork`, `ProgramEvent`, and `ProgramUpdateMode`. Each update records
  the listed edges, H0 edges, algebraic guards, touched atoms, reused atoms,
  and rebuilt atoms charged to that operation.
- Exact one-step class-space continuation. `ClassContinuation` reports
  isomorphism, split, merge, mixing, birth, death, or ambiguity.
  `BasisTransport` is emitted only for equal canonical cocycle vectors. No
  interval or support heuristic creates an individual identity.
- Canonical, bounded `HOLOSPRG` version 1 and `HOLOSDLT` version 1 envelopes.
  A program binds the complete graph, articulation decomposition, composed
  diagram, and one nested proof-carrying atlas per cyclic atom. A delta trace
  embeds every graph and adds a program checkpoint only after repair or
  recompilation. Both verifiers avoid the persistence solver.
- A restricted finite H1 intervention through
  `PersistenceProgram::kill_h1_before`. It returns feasible independent edge
  edits, a checked lower bound, a feasible upper bound, and `Optimal`,
  `BoundedGap`, or `BudgetLimited` status.
- Canonical, bounded `HOLOSINT` version 1 intervention envelopes. Verification
  applies every edge edit, checks the nested delta trace, follows the complete
  target basis through exact transports, and checks status-specific bounds.
- `holos --program`, `holos verify-program`,
  `holos verify-program-trace`, `holos intervene`, and
  `holos verify-intervention`.
- Python `SparseProgram`, `compile_sparse_program`, `load_sparse_program`,
  `compile_sparse_program_trace`, `verify_program_trace`, and
  `verify_intervention`. Program updates expose atoms, exact work, events,
  continuation, artifacts, and interventions.
- Optional `holos_tda.torch.StrictSparseProgram` and
  `finite_h1_intervals`. The adapter provides exact local derivatives of
  finite H1 endpoints with respect to listed edge weights. It rejects ties,
  nonfinite weights, topology changes, and threshold equalities.
- Random differential program tests over Z/2, Z/3, and Z/5. Adversarial tests
  cover proof mutations, truncation, arbitrary bytes, split and merge
  continuation, local repair, topology recompilation, CLI workflows, and
  Python workflows.
- A registered program study with frozen screen and held-out sets. A separate
  deterministic application record checks rank-two space splitting, exact
  basis transport, a reusable trace, and an optimal restricted intervention.

### Changed

- Program H0 evaluation reuses checked forest provenance across independent
  atoms. Cross-atom edge-order changes do not force a global H0 sort.
- Result-sensitive guard evaluation uses compiled edge positions and simplex
  formulas. It does not build a local sparse matrix for each atom during
  diagram-only evaluation.

### Limits

- Programs cover H0 and H1. The ordinary persistence engine remains
  dimension-generic.
- Positive persistence composes only across articulation vertices. A
  separator with two or more vertices needs interface state and remains WIP.
- Result-sensitive guards are sufficient, not a complete description of the
  largest region with the same persistence result. A failed guard triggers an
  exact local rebuild even when another reduction could preserve the result.
- Exact continuation recognizes equal canonical basis vectors for one step.
  Partial equality is ambiguous. The release does not infer longer-path or
  heuristic class identity.
- Intervention supports finite H1 spaces and one destroyer-triangle candidate.
  `Optimal` is relative to the unchanged checked reduction and the maximum
  absolute edge-change norm. It is not a global inverse-persistence result.
- The strict PyTorch adapter is an optional single-trajectory interface. It
  is not batched and does not return a gradient at a tie.
- Program, trace, and intervention proofs can cost more than implicit
  recomputation. Digests bind bytes to graphs but do not authenticate a
  producer or prove that an update policy was followed.

## [0.9.0] - 2026-08-24

### Performance

On the registered held-out trajectory set, serial atlas evaluation was 4.09
times faster than full exact reduction on the 52-point plane entry over Z/3.
It was 4.75 times faster on the 48-point volume entry over Z/5. Each arm
evaluated 40 affine edge-weight updates. The clocks were counterbalanced
across five repetitions after one warm-up. Every diagram matched bit for bit.

Compilation took 1.103 ms and 1.030 ms. The measured update savings recovered
that cost after 30 and 25 steps. The screen rule failed on one entry because
compilation was not recovered within its 25-step trajectory. Both held-out
confirmation entries passed the registered rule.

### Added

- `PersistenceAtlas`, an exact H0 and H1 model for one weak edge-weight order.
  `evaluate_diagram` updates only the diagram. `evaluate` also updates class
  spaces, critical simplices, lineages, and edge-weight sensitivities. Neither
  function runs persistence reduction while the atlas contract holds.
- Exact topology events for vertex changes, listed-edge changes, threshold
  crossings, equality splits, equality merges, and order swaps. `update`
  reuses the current atlas when no event occurs and performs exact fallback
  reduction after an event.
- `PointPersistenceAtlas`, with a conservative per-point Euclidean
  displacement radius and analytic endpoint derivatives by point coordinate.
  The derivative records name the controlling edge and both endpoint terms.
- Persistent H1 class spaces. `IntervalGroupId` names one equal-interval
  space, and `BasisClassId` names one vector in its declared canonical basis.
  Each basis is gauge-fixed modulo vertex coboundaries and then put in sparse
  reduced row-echelon form on the labeled input graph.
- Critical creator edges and destroyer triangles for every positive H1 bar.
  Equal intervals carry the complete sorted set of critical pairs for their
  class space.
- `ReductionCertificate`, an explicit filtered-boundary certificate over any
  supported prime field. Its checker reconstructs each boundary, checks every
  filtration-compatible change-of-basis column, requires distinct reduced
  pivots, and derives the H0 and H1 diagram and critical pairs. It does not
  call the persistence solver.
- Canonical, bounded `HOLOSRED` version 1 and `HOLOSATL` version 1 envelopes.
  An atlas binds the complete listed graph, class spaces, cocycles, critical
  pairs, and a nested reduction certificate. Verification reconstructs a
  ready-to-evaluate atlas without the persistence solver.
- A canonical, bounded `HOLOSTRC` version 1 trajectory envelope. It contains
  every graph and event. Reused steps carry no redundant reduction proof.
  Each region boundary carries a new proof-carrying atlas.
- `holos --atlas FILE`, `holos verify-atlas`, and
  `holos verify-trajectory`. The Python package adds `SparseAtlas`,
  `PointAtlas`, `compile_sparse_atlas`, `load_sparse_atlas`, and
  `compile_points_atlas`.
- Adversarial decoders and randomized differential tests across Z/2, Z/3,
  and Z/5. Finite-difference tests check edge and point gradients. Random
  trajectories compare reuse with full exact reduction at every step.
- A registered trajectory study with frozen screen and held-out confirmation
  entries. The runner rejects any bit-level diagram mismatch before it writes
  a timing record.

### Changed

- Replace flat `PersistentClassId` records with `PersistentClassSpace`,
  `IntervalGroupId`, and `BasisClassId`. Equal persistence intervals no longer
  receive artificial individual identities. There are no compatibility
  aliases.
- Replace the same-solver `RunArtifact` prove profile with independently
  checked proof-carrying atlases. Remove `RunArtifact`, `RunDecodeLimits`,
  `--proof`, and `verify-run` without aliases.
- Representatives JSON now uses `holos-h1-class-spaces-v1`. Each record has
  a space identifier, multiplicity, and canonical basis.

### Limits

- Atlases cover H0 and H1. The ordinary persistence engine remains
  dimension-generic.
- Reuse requires the same vertices, listed edges, threshold memberships, and
  complete weak edge-weight order. An event ends the current lineage. The
  fallback computes a new exact atlas but does not claim a canonical class
  matching across the event.
- A point radius is conservative. It is zero when any pairwise distances are
  tied. Coordinate derivatives are analytic Euclidean derivatives before
  `f64` rounding. A tied endpoint has no single gradient.
- The independent certificate materializes filtered edges, triangles, and
  sparse change-of-basis columns. Production and verification can cost more
  than the implicit engine. `CertificateLimits`, `AtlasDecodeLimits`, and
  `TrajectoryDecodeLimits` bound their resource use.
- Digests bind bytes to supplied graphs. They do not authenticate a producer
  or prove that a run followed a requested optimization or worker schedule.

## [0.8.0] - 2026-08-24

### Performance

On the registered held-out point set, with four workers allowed on four
physical cores and their SMT siblings, exact threshold-native point
construction ran 6.67 times faster than the preserved 0.7 binary on a
low-threshold cube and 2.00 times faster on a mid-threshold sphere. Peak RSS
fell from 26.1 to 4.8 MiB and from 23.3 to 10.3 MiB. Each result is the median
of five fresh-process runs after one warm-up. Every diagram matched exactly.

The registered factorization entry took a 4 ms median with automatic
factorization and 3 ms with it forced or disabled. The automatic arm's 0.75
control ratio is a registered preferred-arm regression. The entry is below a
useful timing scale, but factorization is off by default as a result. Version
0.8 makes no general factorization speed claim.

### Added

- Exact sparse graph construction from point clouds at a finite threshold.
  `PointCloudGraph`, `PointCloudParams`, and `PointCloudStrategy` expose an
  exact k-d-tree radius join and a matrix-free exhaustive fallback. Both
  use the dense path's scaled Euclidean calculation and return the same edge
  values at every worker count. Automatic routing uses the k-d tree through
  12 coordinates.
- Vertex-biconnected factorization for positive-dimensional sparse
  persistence. `GraphFactorization::Auto` selects graphs with at least two
  cyclic blocks when the largest holds at most nine tenths of all cyclic
  edges. H0 runs once on the whole graph. The cyclic blocks run independently
  and share the worker budget. `Off` and `Force` expose both control arms.
- Stable H1 classes and cocycles. `rips_persistence_with_classes` and
  `rips_persistence_with_classes_sparse` return an `ExplainedDiagram`. Each
  positive H1 bar has a canonical cocycle and a SHA-256
  `PersistentClassId`. Finite classes use the largest `f64` below death, and
  essential classes use the terminal filtration level.
- H1 cocycle lifting through collapse algorithm versions 1, 2, and 3. The
  lift verifies the collapse trace, replays its removals in reverse, and
  checks every result on the original graph.
- `holos --representatives FILE`, which writes version 1 JSON records for
  stable H1 classes. The Python package adds `rips_points_classes`,
  `rips_condensed_classes`, and `rips_sparse_classes` with the same records.
- A canonical, resource-bounded H1 run artifact. `RunArtifact` binds the
  thresholded input graph, parameters, diagram, classes, and optional
  collapse artifact. `holos --proof FILE` writes it through a temporary
  sibling file. `holos verify-run INPUT ARTIFACT` checks it in a separate
  process.
- A registered version 0.8 study with frozen screen and confirmation sets.
  It compares the point path with the preserved 0.7 binary and compares all
  three factorization modes. It rejects any diagram mismatch before timing.

### Changed

- Point-cloud commands and Python calls with an explicit finite threshold no
  longer build a condensed distance matrix. They build the exact sparse graph
  directly. Runs without an explicit threshold still use the dense path to
  find the enclosing radius.
- `RipsParams`, the CLI, and the Python functions default `factorization` to
  `GraphFactorization::Off`. Automatic and forced factorization remain
  explicit choices. Explain-profile runs also disable the
  representation-changing reduction shortcuts to keep class identifiers
  stable.
- Python functions add `factorization` after `threads`. Positional calls that
  passed collapse arguments must use the new position or keyword arguments.

### Limits

- Stable representatives cover H1 only. Their identifiers depend on vertex
  labels, the threshold, and the coefficient field. They are not invariant
  under relabeling. Collapse can select another valid cocycle for the same
  interval, so an identifier can also depend on the collapse trace.
- Explain and prove profiles use a fixed whole-graph reduction. They can be
  slower than the compute profile.
- A run artifact is a checked result, not a formal proof or an authenticated
  record. Verification recomputes the diagram and classes with the same
  persistence implementation under a fixed profile. SHA-256 binds bytes to
  the supplied graph but does not identify the producer.

## [0.7.0] - 2026-08-24

### Added

- A deterministic adaptive edge-collapse schedule. Algorithm version 3
  scores all removable edges at the start of each pass. The H1 objective
  ranks triangle removal. The H2 objective ranks tetrahedron removal, then
  triangle removal. The schedule tests each planned removal again against
  the current graph before it commits the removal.
- `AdaptiveCollapseParams`, `CollapseObjective`,
  `collapse_dense_adaptive`, and `collapse_sparse_adaptive` in Rust.
  `RipsParams::with_adaptive_collapse` selects the same path in the
  persistence pipeline. `holos --collapse-schedule adaptive` selects it in
  the CLI. The Python functions accept `collapse_schedule`,
  `collapse_objective`, and `collapse_work_limit`.
- A deterministic work limit for the adaptive schedule. One work unit is
  one complete evaluation of the filtration-wide removal predicate. A run
  never starts a test after it consumes the limit. Its certificate reports
  `CompleteFixedPoint` or `BudgetLimited`, together with the declared limit
  and the consumed work. Both outcomes contain only safe removals.
- A canonical binary collapse artifact. `CollapseArtifact` contains the
  reduced graph and a version 1, 2, or 3 certificate. SHA-256 digests bind
  the thresholded input graph and the reduced graph. The decoder limits
  bytes, vertices, edges, removals, and witness segments before allocation,
  and rejects malformed graphs, noncanonical values, truncation, and
  trailing data.
- `holos --collapse-certificate FILE` writes an artifact through a temporary
  sibling file. `holos verify-collapse INPUT ARTIFACT` checks its graph
  bindings, metadata, witnesses, removals, output graph, and fixed-point
  claim in a separate process.
- A counterbalanced comparative study for complete and budget-limited
  collapse. The frozen corpus separates screen and confirmation seeds. The
  driver checks every diagram bar for bar before timing, verifies every
  artifact, and records phase times, clique counts, work, artifact size,
  and isolated peak memory.

### Changed

- Replace `RemovalStep::epoch()` with `RemovalStep::position()`. The new
  `SchedulePosition` distinguishes a version 1 pass, a version 2 round, and
  a version 3 sequence position. There is no alias for the old method.
- Extend `CollapseCertificate` with its objective, completeness, work limit,
  and consumed work. Historical version 1 and 2 certificates remain
  complete fixed-point certificates and report no adaptive objective.
- Extend independent verification to version 3 sequences and
  budget-limited results. Complete certificates still require a fixed
  point. Budget-limited certificates make no fixed-point claim.

### Limits

- Adaptive collapse is opt-in and runs on one worker. The reduction still
  uses the full worker budget.
- The verifier proves the safety of the recorded trace. It does not prove
  that version 3 followed its score policy, or that any schedule is
  optimal.
- A collapse artifact is not a chain map and does not transport cycle
  representatives. Its digests detect a mismatch with the supplied graph;
  they are not signatures and do not authenticate the producer.

## [0.6.0] - 2026-08-19

### Performance

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

The records behind these numbers are attached to the release: the
confirmation run (claim-grade), the revealed first run on the original
corpus (superseded), and the engineering landing matrix and tuning records
(evidence, not claims).

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

[0.7.0]: https://github.com/t0rsion/holos/releases/tag/v0.7.0
[0.6.0]: https://github.com/t0rsion/holos/releases/tag/v0.6.0
[0.5.0]: https://github.com/t0rsion/holos/releases/tag/v0.5.0
[0.4.0]: https://github.com/t0rsion/holos/releases/tag/v0.4.0
[0.3.1]: https://github.com/t0rsion/holos/releases/tag/v0.3.1
[0.3.0]: https://github.com/t0rsion/holos/releases/tag/v0.3.0
[0.2.1]: https://github.com/t0rsion/holos/releases/tag/v0.2.1
[0.2.0]: https://github.com/t0rsion/holos/releases/tag/v0.2.0
[0.1.0]: https://github.com/t0rsion/holos/releases/tag/v0.1.0
