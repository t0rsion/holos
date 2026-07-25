# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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

[0.1.0]: https://github.com/t0rsion/holos/releases/tag/v0.1.0
