# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.5.0]: https://github.com/t0rsion/holos/releases/tag/v0.5.0
[0.4.0]: https://github.com/t0rsion/holos/releases/tag/v0.4.0
[0.3.1]: https://github.com/t0rsion/holos/releases/tag/v0.3.1
[0.3.0]: https://github.com/t0rsion/holos/releases/tag/v0.3.0
[0.2.1]: https://github.com/t0rsion/holos/releases/tag/v0.2.1
[0.2.0]: https://github.com/t0rsion/holos/releases/tag/v0.2.0
[0.1.0]: https://github.com/t0rsion/holos/releases/tag/v0.1.0
