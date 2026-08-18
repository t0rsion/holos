#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Vietoris-Rips persistent homology over a prime field (Z/2 by default)
//! with an implicit, ripser-style persistent cohomology engine.
//!
//! Tie-breaking and output conventions match ripser exactly. See README.md.

/// The `holos` CLI as a library function (shared with the Python bindings).
pub mod cli;
pub mod collapse;
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
/// The engine is dimension-generic. The differential gates cover
/// `max_dim <= 2`. Stress tests extend through dimension 4.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RipsParams {
    /// Highest homology dimension to compute.
    pub max_dim: usize,
    /// Filtration threshold. `None` means the input's default: the
    /// enclosing radius for dense matrices, no threshold for sparse ones.
    pub threshold: Option<f64>,
    /// Coefficient field Z/p; must be a prime below 32768. Default 2.
    pub modulus: u32,
    /// Worker threads for the run. 0 and 1 (the default) both run the
    /// serial engine. Higher values reduce each dimension concurrently.
    /// With [`RipsParams::collapse_edges`] set and the ordered or rounds
    /// [`RipsParams::collapse_schedule`], the same budget also drives the
    /// collapse: one pool serves the whole pipeline. The diagram is
    /// identical at any thread count.
    pub threads: usize,
    /// Optimization toggle. The diagram is identical with any combination
    /// disabled. For differential testing only.
    pub use_emergent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_apparent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_clearing: bool,
    /// Collapse dominated edges before the engine runs. Off by default.
    /// The diagram is identical either way. See [`collapse`].
    pub collapse_edges: bool,
    /// The schedule the collapse uses with `collapse_edges` set. Default
    /// [`CollapseSchedule::Serial`]. The diagram is identical under every
    /// schedule.
    pub collapse_schedule: CollapseSchedule,
}

/// The collapse the pipeline runs with [`RipsParams::collapse_edges`] set.
///
/// Every schedule gives the same diagram. `Serial` is the default and, in
/// the registered studies, the fastest end to end on most inputs.
/// `Ordered` gives the serial result, bit for bit, from a parallel run.
/// `Rounds` gives a result that does not depend on the worker count and,
/// on some inputs, a smaller reduced graph; its cost grows faster with the
/// edge count than the serial cost. See [`collapse`] for the schedules and
/// their certificates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CollapseSchedule {
    /// The serial schedule on one worker. It writes an algorithm
    /// version 1 certificate. The reduction still uses the whole thread
    /// budget.
    #[default]
    Serial,
    /// The ordered schedule on the run's worker budget. It gives the same
    /// reduced graph and the same certificate as `Serial`.
    Ordered,
    /// The rounds schedule on the run's worker budget. It writes an
    /// algorithm version 2 certificate and gives the same result at every
    /// worker count, but not the serial result.
    Rounds,
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
            collapse_edges: false,
            collapse_schedule: CollapseSchedule::Serial,
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

    /// Collapse dominated edges before the engine runs. The diagram is
    /// identical either way. See [`collapse`].
    pub fn with_edge_collapse(mut self) -> Self {
        self.collapse_edges = true;
        self
    }

    /// Collapse dominated edges with the given schedule before the engine
    /// runs. Also sets [`RipsParams::collapse_edges`]. The diagram is
    /// identical under every schedule.
    pub fn with_collapse_schedule(mut self, schedule: CollapseSchedule) -> Self {
        self.collapse_edges = true;
        self.collapse_schedule = schedule;
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
    if params.collapse_edges {
        return collapse_and_solve(dist, params, |_| {});
    }
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
    if params.collapse_edges {
        return collapse_and_solve(dist, params, |_| {});
    }
    solver::compute(dist, params)
}

/// The collapse pipeline behind [`rips_persistence`]. One run-wide pool
/// serves the selected collapse and then the reduction. The serial
/// schedule collapses before the pool exists, so the pool goes to the
/// reduction alone. Every surviving edge lies at or below the terminal
/// level, so the terminal level is the exact threshold for the reduced
/// complex. `report` sees the collapse result before the reduction
/// starts, which is how the CLI prints its statistics without building a
/// second pool.
pub(crate) fn collapse_and_solve<D: distances::Distances + Sync>(
    dist: &D,
    params: &RipsParams,
    report: impl FnOnce(&collapse::CollapsedRips),
) -> Result<Diagram> {
    let build_pool = || -> Result<Option<rayon::ThreadPool>> {
        if params.threads > 1 {
            Ok(Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(params.threads)
                    .build()
                    .map_err(|e| Error::Io(format!("thread pool: {e}")))?,
            ))
        } else {
            Ok(None)
        }
    };
    let (collapsed, pool) = match params.collapse_schedule {
        CollapseSchedule::Serial => {
            let collapsed = collapse::collapse_serial_in(dist, params.threshold)?;
            (collapsed, build_pool()?)
        }
        CollapseSchedule::Ordered => {
            let pool = build_pool()?;
            let collapsed = collapse::collapse_ordered_in(dist, params.threshold, pool.as_ref())?;
            (collapsed, pool)
        }
        CollapseSchedule::Rounds => {
            let pool = build_pool()?;
            let collapsed = collapse::collapse_rounds_in(dist, params.threshold, pool.as_ref())?;
            (collapsed, pool)
        }
    };
    report(&collapsed);
    let mut inner = params.clone();
    inner.collapse_edges = false;
    inner.threshold = Some(collapsed.certificate.terminal_level());
    solver::compute_in(&collapsed.matrix, &inner, pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collapse::verify::verify_dense;

    fn grid(side: usize) -> DistanceMatrix {
        let mut points = Vec::new();
        for i in 0..side {
            for j in 0..side {
                points.push(vec![i as f64, j as f64]);
            }
        }
        DistanceMatrix::from_points(&points).unwrap()
    }

    #[test]
    fn pipeline_runs_the_selected_schedule() {
        // The diagram is the same under every schedule, so only the
        // certificate the pipeline hands to `report` shows which collapse
        // ran: version 2 for rounds, version 1 otherwise, and the ordered
        // run at four workers tests more edges than the serial one on this
        // grid. Every certificate must pass the independent verifier.
        let dist = grid(5);
        let plain = rips_persistence(&dist, &RipsParams::new(2)).unwrap();
        let mut seen = Vec::new();
        for schedule in [
            CollapseSchedule::Serial,
            CollapseSchedule::Ordered,
            CollapseSchedule::Rounds,
        ] {
            let params = RipsParams::new(2)
                .with_threads(4)
                .with_collapse_schedule(schedule);
            let mut captured = None;
            let diagram =
                collapse_and_solve(&dist, &params, |c| captured = Some(c.clone())).unwrap();
            let captured = captured.expect("report must see the collapse");
            verify_dense(&dist, None, &captured).unwrap();
            let expected_version = if schedule == CollapseSchedule::Rounds {
                2
            } else {
                1
            };
            assert_eq!(captured.certificate.algorithm_version(), expected_version);
            let mut a = diagram.clone();
            let mut b = plain.clone();
            a.canonicalize();
            b.canonicalize();
            assert_eq!(a.bars, b.bars, "{schedule:?}");
            seen.push((schedule, captured.stats));
        }
        let serial = seen[0].1;
        let ordered = seen[1].1;
        assert_eq!(ordered.logical_tests, serial.edge_tests);
        assert!(
            ordered.edge_tests > serial.edge_tests,
            "the ordered schedule did not speculate: {} vs {}",
            ordered.edge_tests,
            serial.edge_tests
        );
        assert_eq!(serial.window_batches, 0);
        assert!(ordered.window_batches > 0);
    }
}
