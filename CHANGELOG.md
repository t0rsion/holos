# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.3.1]: https://github.com/t0rsion/holos/releases/tag/v0.3.1
[0.3.0]: https://github.com/t0rsion/holos/releases/tag/v0.3.0
[0.2.1]: https://github.com/t0rsion/holos/releases/tag/v0.2.1
[0.2.0]: https://github.com/t0rsion/holos/releases/tag/v0.2.0
[0.1.0]: https://github.com/t0rsion/holos/releases/tag/v0.1.0
