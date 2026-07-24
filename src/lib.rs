#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Vietoris-Rips persistent homology over Z/2 with an implicit,
//! ripser-style persistent cohomology engine.
//!
//! Tie-breaking and output conventions match ripser exactly; see README.md.

pub(crate) mod combinadic;
/// Distance-matrix construction and storage.
pub mod distances;
/// File formats and diagram output.
pub mod io;
/// Independent brute-force reference implementation used by the test gates.
pub mod oracle;
pub(crate) mod solver;
mod union_find;

use std::fmt;

pub use distances::DistanceMatrix;

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
    /// then death. Deterministic across runs and point permutations.
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
/// The engine is dimension-generic; v0.1's differential gates certify
/// `max_dim <= 2` (review stress-testing extended through dimension 4).
#[derive(Debug, Clone)]
pub struct RipsParams {
    /// Highest homology dimension to compute.
    pub max_dim: usize,
    /// None means the enclosing radius (exactness-preserving default).
    pub threshold: Option<f64>,
    /// Debug toggle; output is identical with any combination disabled.
    pub use_emergent_pairs: bool,
    /// Debug toggle; output is identical with any combination disabled.
    pub use_apparent_pairs: bool,
    /// Debug toggle; output is identical with any combination disabled.
    pub use_clearing: bool,
}

impl Default for RipsParams {
    fn default() -> Self {
        Self {
            max_dim: 1,
            threshold: None,
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
