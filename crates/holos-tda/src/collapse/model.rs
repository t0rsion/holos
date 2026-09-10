use crate::SparseDistanceMatrix;

/// The downstream work an adaptive collapse schedule targets.
///
/// In each pass, `H1` favors removals that destroy more triangles. `H2`
/// first favors removals that destroy more tetrahedra, then uses the
/// triangle count as a tie breaker. Each planned removal is tested again
/// against the current graph. The score guides the order only. Every
/// removal passes the same filtration-wide predicate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollapseObjective {
    /// Target the cofacets used most directly by an H1 computation.
    H1,
    /// Target H2 cofacets, then H1 cofacets.
    H2,
}

/// Why a collapse run stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CollapseCompleteness {
    /// The output has no edge that passes the collapse predicate.
    CompleteFixedPoint,
    /// The declared work limit stopped the run before a fixed-point check.
    BudgetLimited,
}

/// Parameters for the adaptive version 3 collapse schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct AdaptiveCollapseParams {
    /// Downstream work used to rank removable edges.
    pub objective: CollapseObjective,
    /// Maximum removability tests. `None` runs to a fixed point.
    ///
    /// One work unit is one complete evaluation of the filtration-wide
    /// edge predicate, including score construction when the edge is
    /// removable. A run never starts a test after it consumes this limit.
    pub work_limit: Option<u64>,
}

impl Default for AdaptiveCollapseParams {
    fn default() -> Self {
        Self {
            objective: CollapseObjective::H2,
            work_limit: None,
        }
    }
}

impl AdaptiveCollapseParams {
    /// Run the adaptive schedule to a fixed point with `objective`.
    pub fn new(objective: CollapseObjective) -> Self {
        Self {
            objective,
            work_limit: None,
        }
    }

    /// Stop before starting a predicate evaluation beyond `work_limit`.
    pub fn with_work_limit(mut self, work_limit: u64) -> Self {
        self.work_limit = Some(work_limit);
        self
    }
}

/// Where one removal sits in its schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SchedulePosition {
    /// A serial or ordered version 1 pass.
    Pass(usize),
    /// A rounds-schedule version 2 round.
    Round(usize),
    /// An unstructured version 3 removal sequence.
    Sequence(usize),
}

impl SchedulePosition {
    /// The 1-based pass, round, or sequence number.
    pub fn number(self) -> usize {
        match self {
            Self::Pass(number) | Self::Round(number) | Self::Sequence(number) => number,
        }
    }
}

/// One removed edge: endpoints, original value, schedule position, and the
/// piecewise witness function.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalStep {
    pub(super) u: usize,
    pub(super) v: usize,
    pub(super) value: f64,
    pub(super) position: SchedulePosition,
    pub(super) witnesses: Vec<(f64, usize)>,
}

impl RemovalStep {
    /// Original endpoints, smaller index first.
    pub fn edge(&self) -> (usize, usize) {
        (self.u, self.v)
    }

    /// Original edge value, preserved bit for bit.
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The 1-based schedule position of the removal.
    pub fn position(&self) -> SchedulePosition {
        self.position
    }

    /// Witness segments as `(start value, apex vertex)`. Segment `i` covers
    /// scales from its start value up to the next segment's start; the last
    /// segment covers through the terminal level. The first start value is
    /// the edge value.
    pub fn witnesses(&self) -> &[(f64, usize)] {
        &self.witnesses
    }
}

/// Replayable record of one collapse run.
///
/// The certificate plus the collapsed matrix reconstruct the thresholded
/// input. The certificate is not a chain map and does not transport
/// representatives.
#[derive(Debug, Clone, PartialEq)]
pub struct CollapseCertificate {
    pub(super) algorithm_version: u32,
    pub(super) objective: Option<CollapseObjective>,
    pub(super) completeness: CollapseCompleteness,
    pub(super) work_limit: Option<u64>,
    pub(super) work_used: u64,
    pub(super) vertex_count: usize,
    pub(super) requested_threshold: Option<f64>,
    pub(super) terminal_level: f64,
    pub(super) input_edge_count: usize,
    pub(super) output_edge_count: usize,
    pub(super) steps: Vec<RemovalStep>,
}

impl CollapseCertificate {
    /// Version of the collapse scheme that produced this certificate.
    pub fn algorithm_version(&self) -> u32 {
        self.algorithm_version
    }

    /// Downstream objective for an adaptive version 3 run.
    ///
    /// Versions 1 and 2 return `None`. Those schedules do not rank
    /// removals by a downstream-work estimate.
    pub fn objective(&self) -> Option<CollapseObjective> {
        self.objective
    }

    /// Whether the output is a fixed point or a safe partial collapse.
    pub fn completeness(&self) -> CollapseCompleteness {
        self.completeness
    }

    /// Declared adaptive work limit, when the caller set one.
    pub fn work_limit(&self) -> Option<u64> {
        self.work_limit
    }

    /// Adaptive work units consumed by the schedule.
    ///
    /// Versions 1 and 2 report zero.
    pub fn work_used(&self) -> u64 {
        self.work_used
    }

    /// Number of vertices in the input.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// The threshold the caller passed, verbatim.
    pub fn requested_threshold(&self) -> Option<f64> {
        self.requested_threshold
    }

    /// Terminal filtration level: the resolved threshold if finite,
    /// otherwise the largest finite edge value.
    pub fn terminal_level(&self) -> f64 {
        self.terminal_level
    }

    /// Edges in the thresholded input.
    pub fn input_edge_count(&self) -> usize {
        self.input_edge_count
    }

    /// Edges that survived the collapse.
    pub fn output_edge_count(&self) -> usize {
        self.output_edge_count
    }

    /// The removals, in execution order.
    pub fn steps(&self) -> &[RemovalStep] {
        &self.steps
    }
}

/// Counters from one collapse run.
///
/// The structural fields (`input_edges`, `output_edges`, `removed_edges`,
/// `epochs`, `witness_segments`, `logical_tests`) are the same at every
/// worker count and window. The other fields describe one execution:
/// `edge_tests`, `max_common_neighborhood`, and the fields marked
/// "ordered schedule only" can differ between ordered runs with different
/// worker counts or windows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CollapseStats {
    /// Edges in the thresholded input.
    pub input_edges: usize,
    /// Edges that survived.
    pub output_edges: usize,
    /// Edges removed.
    pub removed_edges: usize,
    /// Schedule epochs, including the final epoch that removes nothing:
    /// passes for versions 1 and 3, rounds for version 2. A budget-limited
    /// version 3 run can stop during its last pass.
    pub epochs: usize,
    /// Predicate evaluations, physical calls. On the ordered schedule
    /// this depends on the worker count and window and can exceed
    /// `logical_tests`; on the other schedules it equals it.
    pub edge_tests: usize,
    /// Witness segments recorded across all removal steps.
    pub witness_segments: usize,
    /// Largest common-neighborhood size seen by the predicate. On the
    /// ordered schedule a discarded speculative test can see a larger
    /// neighborhood than the serial run tests against, so this field
    /// depends on the worker count and window.
    pub max_common_neighborhood: usize,
    /// Tests of the schedule's own trace. Equals `edge_tests` for the
    /// serial and rounds schedules; for the ordered schedule it is the
    /// serial trace's test count, and it does not depend on the worker
    /// count or window.
    pub logical_tests: usize,
    /// Cached speculative results dropped before use, whether a
    /// conflicting removal or a large-neighborhood bail invalidated them.
    /// Each dropped result is re-evaluated serially at its turn. Ordered
    /// schedule only.
    pub invalidated_results: usize,
    /// Large-neighborhood marking bails during retirement that dropped at
    /// least one cached result ahead of them. Ordered schedule only.
    pub global_invalidations: usize,
    /// Speculative window stages executed. Ordered schedule only.
    pub window_batches: usize,
    /// Window slots offered across all stages: stages times the window
    /// size. With `window_members_formed` it gives window occupancy.
    /// Ordered schedule only.
    pub window_slots_offered: usize,
    /// Positions collected into windows across all stages. Ordered
    /// schedule only.
    pub window_members_formed: usize,
    /// Cached member verdicts consumed at retirement without a repair.
    /// Ordered schedule only.
    pub window_members_reused: usize,
    /// Score evaluations by the adaptive schedule. This includes pass
    /// planning and successful retirement tests.
    pub adaptive_score_evaluations: usize,
    /// Planned removals considered in score order by the adaptive schedule.
    pub adaptive_queue_pops: usize,
    /// Planned removals that failed their retirement test after an earlier
    /// removal changed the graph.
    pub adaptive_stale_pops: usize,
    /// Triangles destroyed by the removals selected by the adaptive
    /// schedule, counted immediately before each removal.
    pub adaptive_triangles_removed: u64,
    /// Tetrahedra destroyed by the removals selected by the adaptive
    /// schedule, counted immediately before each removal.
    pub adaptive_tetrahedra_removed: u64,
}

impl CollapseStats {
    /// Counters with `input_edges` set and every input edge still an output edge.
    pub(super) fn new(input_edges: usize) -> Self {
        CollapseStats {
            input_edges,
            output_edges: input_edges,
            ..Default::default()
        }
    }
}

/// Wall-clock split of one collapse run, in nanoseconds.
///
/// Zero on the serial and rounds schedules, which have no speculative
/// phases to separate. The ordered schedule reports the parallel test
/// phase, the serial retirement walk, and the repairs inside it. Timings
/// are diagnostics: they vary between runs and never affect an output
/// field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct CollapseTimings {
    /// Time in the parallel test phase, summed over stages.
    pub predicate_ns: u64,
    /// Time in the serial retirement walk, summed over stages. Includes
    /// the repairs counted in `repair_ns`.
    pub retirement_ns: u64,
    /// Time spent re-evaluating invalidated verdicts at their turn.
    pub repair_ns: u64,
}

/// Reduced graph, collapse certificate, run counters, and wall-clock timings.
///
/// The matrix is the input to [`crate::rips_persistence_sparse`]. The
/// reduction does not depend on modulus, homology dimension, optimization
/// toggles, or solver thread count. The schedule does change which edges
/// survive.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct CollapsedRips {
    /// The reduced graph. Surviving edge values are the input values, bit
    /// for bit.
    pub matrix: SparseDistanceMatrix,
    /// Replayable proof of every removal.
    pub certificate: CollapseCertificate,
    /// Run counters.
    pub stats: CollapseStats,
    /// Wall-clock split of the run. Diagnostics only.
    pub timings: CollapseTimings,
}
