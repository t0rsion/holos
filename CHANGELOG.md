# Changelog

All notable changes to this project are documented in this file. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
