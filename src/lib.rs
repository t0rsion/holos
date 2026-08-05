#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Vietoris-Rips persistent homology over a prime field (Z/2 by default)
//! with an implicit, ripser-style persistent cohomology engine.
//!
//! Tie-breaking and output conventions match ripser exactly. See README.md.

/// The `holos` CLI as a library function (shared with the Python bindings).
pub mod cli;
pub(crate) mod combinadic;
/// Distance-matrix construction and storage.
pub mod distances;
pub(crate) mod field;
/// File formats and diagram output.
pub mod io;
/// Independent brute-force reference implementation used by the test gates.
pub mod oracle;
pub(crate) mod parallel;
pub(crate) mod reduce;
pub(crate) mod simplex;
pub(crate) mod solver;
mod union_find;

use std::fmt;

pub use distances::{DistanceMatrix, SparseDistanceMatrix};

/// Short git commit hash recorded at build time ("unknown" outside a repo).
pub const GIT_HASH: &str = env!("HOLOS_GIT_HASH");
/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Cargo build profile recorded at build time.
pub const BUILD_PROFILE: &str = env!("HOLOS_BUILD_PROFILE");

/// One persistence interval. `death` is `f64::INFINITY` for essential classes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bar {
    /// Homology dimension.
    pub dim: usize,
    /// Filtration value at which the class appears.
    pub birth: f64,
    /// Filtration value at which the class dies.
    pub death: f64,
}

impl Bar {
    /// True when the class never dies.
    pub fn is_essential(&self) -> bool {
        self.death == f64::INFINITY
    }
}

/// A persistence diagram: the multiset of bars across dimensions.
#[derive(Debug, Clone, Default)]
pub struct Diagram {
    /// All bars, in canonical order after [`Diagram::canonicalize`].
    pub bars: Vec<Bar>,
}

impl Diagram {
    /// Bars of one homology dimension.
    pub fn in_dim(&self, dim: usize) -> impl Iterator<Item = &Bar> {
        self.bars.iter().filter(move |b| b.dim == dim)
    }

    /// Sort bars into the canonical output order: by dimension, then birth,
    /// then death. The order is deterministic across runs and point
    /// permutations.
    pub fn canonicalize(&mut self) {
        self.bars.sort_by(|a, b| {
            a.dim
                .cmp(&b.dim)
                .then(a.birth.total_cmp(&b.birth))
                .then(a.death.total_cmp(&b.death))
        });
    }
}

/// Parameters for [`rips_persistence`].
///
/// The engine is dimension-generic. The differential gates certify
/// `max_dim <= 2`. Stress tests extend through dimension 4.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RipsParams {
    /// Highest homology dimension to compute.
    pub max_dim: usize,
    /// None means the input's default: the enclosing radius for dense
    /// matrices, no threshold for sparse ones.
    pub threshold: Option<f64>,
    /// Coefficient field Z/p; must be a prime below 32768. Default 2.
    pub modulus: u32,
    /// Worker threads for the reduction. 0 and 1 (the default) both run the
    /// serial engine. Higher values reduce each dimension concurrently. The
    /// diagram is identical at any thread count.
    pub threads: usize,
    /// Optimization toggle. The diagram is identical with any combination
    /// disabled. For differential testing only.
    pub use_emergent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_apparent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_clearing: bool,
}

impl Default for RipsParams {
    fn default() -> Self {
        Self {
            max_dim: 1,
            threshold: None,
            modulus: 2,
            threads: 1,
            use_emergent_pairs: true,
            use_apparent_pairs: true,
            use_clearing: true,
        }
    }
}

impl RipsParams {
    /// Defaults with the given `max_dim`: enclosing-radius threshold, all
    /// optimizations on.
    pub fn new(max_dim: usize) -> Self {
        Self {
            max_dim,
            ..Self::default()
        }
    }

    /// Truncate the filtration at `threshold`.
    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.threshold = Some(threshold);
        self
    }

    /// Compute over Z/p instead of Z/2. `modulus` must be a prime below
    /// 32768.
    pub fn with_modulus(mut self, modulus: u32) -> Self {
        self.modulus = modulus;
        self
    }

    /// Reduce with `threads` workers. 1 keeps the serial engine. The diagram
    /// is identical at any thread count.
    pub fn with_threads(mut self, threads: usize) -> Self {
        self.threads = threads.max(1);
        self
    }
}

/// Errors surfaced by construction, validation, and IO.
#[derive(Debug, Clone, PartialEq)]
#[allow(missing_docs)]
pub enum Error {
    InvalidDistance(String),
    InvalidInput(String),
    IndexOverflow { n: usize, dim: usize },
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidDistance(msg) => write!(f, "invalid distance: {msg}"),
            Error::InvalidInput(msg) => write!(f, "invalid input: {msg}"),
            Error::IndexOverflow { n, dim } => write!(
                f,
                "simplex index space overflows u64 for {n} points in dimension {dim}"
            ),
            Error::Io(msg) => write!(f, "io error: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// Crate-wide result alias.
pub type Result<T> = std::result::Result<T, Error>;

/// Compute the Rips persistence diagram of a distance matrix.
pub fn rips_persistence(dist: &DistanceMatrix, params: &RipsParams) -> Result<Diagram> {
    solver::compute(dist, params)
}

/// Compute the Rips persistence diagram of a sparse distance matrix.
///
/// Pairs not listed in the input are absent at every scale. With no
/// threshold set, all listed edges enter the filtration.
pub fn rips_persistence_sparse(
    dist: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Result<Diagram> {
    solver::compute(dist, params)
}
