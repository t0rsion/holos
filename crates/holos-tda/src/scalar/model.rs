use std::fmt;

use crate::collapse;

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

/// Parameters for [`crate::rips_persistence`].
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
    /// [`RipsParams::collapse_schedule`], `threads` is the budget for the
    /// whole pipeline. The diagram is identical at any thread count.
    pub threads: usize,
    /// Optimization toggle. The diagram is identical with any combination
    /// disabled. For differential testing only.
    pub use_emergent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_apparent_pairs: bool,
    /// See [`RipsParams::use_emergent_pairs`].
    pub use_clearing: bool,
    /// See [`RipsParams::use_emergent_pairs`]. With this set, and the graph
    /// dense enough for the rows to fit their memory budget, the dim-0
    /// apparent test runs on adjacency bitsets instead of the neighbor
    /// lists.
    pub use_adjacency_rows: bool,
    /// Collapse dominated edges before the engine runs. Off by default.
    /// The diagram is identical either way. See [`collapse`].
    pub collapse_edges: bool,
    /// The schedule the collapse uses with `collapse_edges` set. Default
    /// [`CollapseSchedule::Serial`]. See [`CollapseSchedule`].
    pub collapse_schedule: CollapseSchedule,
    /// Objective and deterministic work limit for
    /// [`CollapseSchedule::Adaptive`]. Other schedules ignore this field.
    pub adaptive_collapse: collapse::AdaptiveCollapseParams,
    /// Which engine reduces a dense input. Default [`Engine::Auto`]. See
    /// [`Engine`].
    pub engine: Engine,
    /// Which storage form the dense engine reduces from. Default
    /// [`DenseStorage::Auto`]. See [`DenseStorage`].
    pub dense_storage: DenseStorage,
    /// Structural decomposition of a sparse terminal graph. Default
    /// [`GraphFactorization::Off`]. Dense runs use it only after routing to
    /// the sparse engine. See [`GraphFactorization`].
    pub factorization: GraphFactorization,
}

/// Which engine reduces a dense input.
///
/// A dense matrix can be reduced as it stands, or converted to the graph
/// of its edges at the threshold and reduced by the sparse engine. The
/// second is faster when few pairs are edges, because the sparse
/// enumerator walks a neighbor list where the dense one scans every
/// vertex. `Auto` picks between them from the edge density at the resolved
/// threshold and from the memory the conversion would take; `Dense` and
/// `Sparse` force one.
///
/// The diagram is identical under all three, bit for bit. Every simplex of
/// the complex has diameter at most the threshold, and a diameter is the
/// largest of the edge lengths, so every edge of every simplex survives
/// the conversion. The conversion keeps only edges at or below the
/// threshold. The vertex set carries over because the conversion passes
/// the point count explicitly.
///
/// An infinite threshold reads as `f64::MAX` in both engines. An absent
/// pair has distance `+inf`, so it enters neither complex, and the
/// conversion keeps exactly the pairs the dense engine admits.
///
/// The rule and its constants were frozen on 2026-08-18 from disclosed
/// engineering data. Performance assessment is WIP.
///
/// A sparse input is never routed, so this setting does not reach
/// [`crate::rips_persistence_sparse`]. With [`RipsParams::collapse_edges`] set,
/// the collapse already produces a graph and the sparse engine reduces it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Engine {
    /// Reduce a low-density dense input with the sparse engine, and every
    /// other dense input with the dense engine. The rule takes no argument.
    /// The conversion has a memory budget: 32 MiB, or the bytes of the
    /// compact matrix, whichever is larger. A matrix of mostly absent
    /// pairs is low-density at any threshold, including an infinite one,
    /// so it routes too.
    #[default]
    Auto,
    /// Always reduce the distance matrix as it stands.
    Dense,
    /// Always convert to the thresholded graph and reduce that. The
    /// conversion costs one pass over the matrix. This is an explicit
    /// request, so the `Auto` memory budget does not apply.
    Sparse,
}

/// Which storage form the dense engine reduces from.
///
/// A [`crate::DistanceMatrix`] is built compact: the condensed lower triangle,
/// `n(n-1)/2` entries. The full form holds both triangles row-major,
/// `n * n` entries, so that the cofacet diameter fold reads a contiguous
/// row per simplex vertex where the compact form reads a strided column.
/// The full form costs `n(n+1)/2` entries more.
///
/// The choice is per run and comes after the routing decision, so a run
/// the router sends to the sparse engine never builds the full form.
/// `Auto` selects it from the compact matrix size, a frozen budget on the
/// added bytes, the edge count at the resolved threshold, and how many
/// distances the fold reads. `Compact` forbids the conversion, which
/// bounds what a run spends on the matrix. `Square` forces it and skips
/// the budget.
///
/// The conversion runs once and the full form lives only as long as the
/// run. The caller keeps its compact matrix, so a run in the full form
/// holds `n * n + n(n-1)/2` entries: one and a half times the full form,
/// three times the compact one.
///
/// The diagram is identical under all three, bit for bit.
///
/// A sparse input holds no distance matrix, so this setting does not reach
/// [`crate::rips_persistence_sparse`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum DenseStorage {
    /// Convert to the full form when the frozen rule selects it, and keep
    /// the compact form otherwise.
    #[default]
    Auto,
    /// Always reduce from the compact form. No run adds the second
    /// triangle.
    Compact,
    /// Always reduce from the full form. This is an explicit request, so
    /// the `Auto` budget on the added bytes does not apply.
    Square,
}

/// Structural routing for positive-dimensional sparse persistence.
///
/// Every terminal edge belongs to one vertex-biconnected block. Every
/// terminal clique with at least two vertices lies in one such block, and
/// every positive-dimensional cycle splits over the blocks. The engine can
/// therefore compute H0 once on the whole graph and compute H1 and above on
/// the cyclic blocks independently.
///
/// The diagram is identical under every setting. The automatic rule uses
/// factorization only when there are at least two cyclic blocks and the
/// largest holds at most nine tenths of their edges. A graph with one
/// dominant block stays on the existing reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum GraphFactorization {
    /// Use the frozen structural rule.
    Auto,
    /// Reduce the whole terminal graph.
    #[default]
    Off,
    /// Split every terminal graph.
    Force,
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
    /// The adaptive version 3 schedule. It ranks currently valid removals
    /// by estimated downstream H1 or H2 work and can stop at a declared
    /// work limit. It runs serially; the reduction still uses the whole
    /// thread budget.
    Adaptive,
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
            use_adjacency_rows: true,
            collapse_edges: false,
            collapse_schedule: CollapseSchedule::Serial,
            adaptive_collapse: collapse::AdaptiveCollapseParams::default(),
            engine: Engine::Auto,
            dense_storage: DenseStorage::Auto,
            factorization: GraphFactorization::Off,
        }
    }
}

impl RipsParams {
    /// Defaults with the given `max_dim`.
    ///
    /// The threshold is the input's default. Reduction shortcuts are on.
    /// Edge collapse and structural factorization are off.
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
    /// runs. Also sets [`RipsParams::collapse_edges`]. See
    /// [`CollapseSchedule`].
    pub fn with_collapse_schedule(mut self, schedule: CollapseSchedule) -> Self {
        self.collapse_edges = true;
        self.collapse_schedule = schedule;
        self
    }

    /// Collapse with the adaptive version 3 schedule and the given
    /// objective and work limit.
    pub fn with_adaptive_collapse(mut self, params: collapse::AdaptiveCollapseParams) -> Self {
        self.collapse_edges = true;
        self.collapse_schedule = CollapseSchedule::Adaptive;
        self.adaptive_collapse = params;
        self
    }

    /// Choose the engine for a dense input. See [`Engine`].
    pub fn with_engine(mut self, engine: Engine) -> Self {
        self.engine = engine;
        self
    }

    /// Choose the storage form the dense engine reduces from. See
    /// [`DenseStorage`].
    pub fn with_dense_storage(mut self, storage: DenseStorage) -> Self {
        self.dense_storage = storage;
        self
    }

    /// Choose structural factorization for sparse reduction. See
    /// [`GraphFactorization`].
    pub fn with_factorization(mut self, factorization: GraphFactorization) -> Self {
        self.factorization = factorization;
        self
    }
}

/// Errors from construction, validation, and IO.
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
