//! Filtered edge collapse for flag filtrations.
//!
//! The collapse removes edges that are dominated at every scale from their
//! birth to the terminal level (the filtration-wide multi-witness criterion
//! of Boissonnat and Pritam). The flag filtration of the reduced graph has
//! the same persistence diagram as the input, in every dimension. Each
//! removal is recorded in a replayable [`CollapseCertificate`] that the
//! independent checker in [`verify`] can validate.
//!
//! Four schedules exist. [`collapse_dense`] and [`collapse_sparse`] run
//! the serial schedule: passes over the edges with immediate deletion.
//! This is what [`crate::rips_persistence`] and the CLI run by default,
//! and in the registered studies the fastest end to end on most inputs;
//! [`crate::CollapseSchedule`] selects the others.
//! [`collapse_dense_ordered_parallel`] and
//! [`collapse_sparse_ordered_parallel`] run the ordered schedule: the same
//! removals, tested speculatively in parallel, so their output is the
//! serial one, field for field, at every worker count. Both schedules
//! write an algorithm version 1 certificate.
//! [`collapse_dense_rounds_parallel`] and
//! [`collapse_sparse_rounds_parallel`] run the rounds schedule, which
//! writes a version 2 certificate: each round tests the live edges against
//! a frozen graph and deletes a batch of provably independent removals,
//! also byte-identical at every worker count. All are deterministic given
//! the vertex labeling. [`collapse_dense_adaptive`] and
//! [`collapse_sparse_adaptive`] run the version 3 schedule. It ranks live
//! removals by the triangles or tetrahedra they remove, and can return a
//! certified partial collapse at a declared work limit. No reduced graph
//! is canonical. A relabeling or schedule change can move the surviving
//! set, but never the barcode.

mod adaptive;
mod ordered;
mod parallel;
pub mod verify;
/// Portable collapse certificates and their reduced graphs.
pub mod wire;

pub(crate) use adaptive::collapse_adaptive_in;
pub use adaptive::{collapse_dense_adaptive, collapse_sparse_adaptive};
pub(crate) use ordered::collapse_ordered_in;
pub use ordered::{
    collapse_dense_ordered_parallel, collapse_dense_ordered_with_window,
    collapse_sparse_ordered_parallel, collapse_sparse_ordered_with_window,
};
pub(crate) use parallel::collapse_rounds_in;
pub use parallel::{collapse_dense_rounds_parallel, collapse_sparse_rounds_parallel};

use crate::distances::Distances;
use crate::{DistanceMatrix, Error, Result, SparseDistanceMatrix};

/// The downstream work an adaptive collapse schedule targets.
///
/// In each pass, `H1` favors removals that destroy more triangles. `H2`
/// first favors removals that destroy more tetrahedra, then uses the
/// triangle count as a tie breaker. Each planned removal is tested again
/// against the current graph. The score guides the order only. Every
/// removal passes the same filtration-wide predicate and preserves
/// persistence in every dimension.
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
    /// A snapshot-parallel version 2 round.
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
/// piecewise witness function that certifies the removal.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalStep {
    u: usize,
    v: usize,
    value: f64,
    position: SchedulePosition,
    witnesses: Vec<(f64, usize)>,
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
/// input, and [`verify`] can replay and check every removal. The
/// certificate is not a chain map: it certifies that the removals preserve
/// the diagram, and does not transport representatives.
#[derive(Debug, Clone, PartialEq)]
pub struct CollapseCertificate {
    algorithm_version: u32,
    objective: Option<CollapseObjective>,
    completeness: CollapseCompleteness,
    work_limit: Option<u64>,
    work_used: u64,
    vertex_count: usize,
    requested_threshold: Option<f64>,
    terminal_level: f64,
    input_edge_count: usize,
    output_edge_count: usize,
    steps: Vec<RemovalStep>,
}

impl CollapseCertificate {
    /// Version of the collapse scheme that produced this certificate.
    pub fn algorithm_version(&self) -> u32 {
        self.algorithm_version
    }

    /// Downstream objective for an adaptive version 3 run.
    ///
    /// Versions 1 and 2 return `None` because their schedules do not rank
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
    /// Versions 1 and 2 report zero because their historical certificates
    /// did not define this counter.
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

    /// The level through which every removal is certified: the resolved
    /// threshold if finite, otherwise the largest finite edge value.
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
    /// Every dropped result runs again serially at its turn, so this is
    /// also the repair count. Ordered schedule only.
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
    /// Counters before the first schedule stage: nothing tested, nothing removed,
    /// and every input edge still an output edge.
    fn new(input_edges: usize) -> Self {
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
/// phase, the serial retirement walk, and the repairs inside it, so a
/// study can weigh repair cost against predicate cost directly. Timings
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

/// A collapsed filtration: the reduced graph, the certificate that the
/// reduction preserves the diagram, run counters, and wall-clock timings.
///
/// Pass the matrix straight to [`crate::rips_persistence_sparse`]. One
/// collapse can serve many downstream runs: the reduction is independent of
/// modulus, homology dimension, optimization toggles, and thread count.
/// The schedule does change which edges survive.
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

/// Collapse a dense distance matrix with the serial schedule.
///
/// `threshold` follows the engine's rule: `None` means the enclosing
/// radius. Edges above the resolved threshold are dropped before the
/// collapse and are not part of the certified input. The parallel forms
/// are [`collapse_dense_ordered_parallel`] and
/// [`collapse_dense_rounds_parallel`].
pub fn collapse_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> Result<CollapsedRips> {
    collapse_impl(dist, threshold)
}

/// Collapse a sparse distance matrix with the serial schedule.
///
/// `threshold` follows the engine's rule: `None` keeps every listed edge.
/// The parallel forms are [`collapse_sparse_ordered_parallel`] and
/// [`collapse_sparse_rounds_parallel`].
pub fn collapse_sparse(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> Result<CollapsedRips> {
    collapse_impl(dist, threshold)
}

/// One edge of the thresholded input, held in schedule order. Removal
/// clears `alive` and tombstones the two adjacency entries.
struct EdgeRec {
    u: usize,
    v: usize,
    value: f64,
    alive: bool,
}

/// An adjacency entry: neighbor, current value (+inf once tombstoned), and
/// the position of the edge in the schedule array.
type AdjEntry = (usize, f64, usize);

/// The certificate header of a run: the vertex count, the level through
/// which removals are certified, and the threshold the caller passed.
struct Run {
    n: usize,
    terminal: f64,
    threshold: Option<f64>,
}

/// The thresholded input of one collapse.
struct Prepared {
    /// The edges in schedule order.
    edges: Vec<EdgeRec>,
    /// Adjacency lists over `edges`, each in ascending neighbor order.
    adj: Vec<Vec<AdjEntry>>,
    /// What the certificate reports besides the edges.
    run: Run,
}

/// Which execution produced a run. It fixes the two output fields the
/// three executions do not share.
#[derive(Clone, Copy)]
enum Execution {
    /// The serial version 1 run: certificate version 1, and the physical
    /// test sequence is the logical one.
    Serial,
    /// The ordered speculative version 1 run: certificate version 1, and
    /// the execution counts its own logical trace, which the physical
    /// count can exceed.
    Ordered,
    /// The version 2 rounds schedule: certificate version 2, and the
    /// physical test sequence is the logical one.
    Snapshot,
    /// The adaptive version 3 schedule and its declared stopping state.
    Adaptive {
        objective: CollapseObjective,
        completeness: CollapseCompleteness,
        work_limit: Option<u64>,
        work_used: u64,
    },
}

/// Collect the thresholded input and index it. The three executions start
/// here, so they see the same edge order and the same terminal level.
fn prepare<D: Distances>(dist: &D, threshold: Option<f64>) -> Result<Prepared> {
    validate_threshold(threshold)?;
    let n = dist.len();
    let resolved = threshold.unwrap_or_else(|| dist.default_threshold());

    let mut edges: Vec<EdgeRec> = Vec::new();
    dist.for_each_edge(|i, j, d| {
        if d.is_finite() && d <= resolved {
            edges.push(EdgeRec {
                u: j,
                v: i,
                value: d,
                alive: true,
            });
        }
    });
    let terminal = if resolved.is_finite() {
        resolved
    } else {
        edges.iter().map(|e| e.value).fold(0.0f64, f64::max)
    };

    edges.sort_unstable_by(|a, b| {
        b.value
            .total_cmp(&a.value)
            // The combinadic edge index v*(v-1)/2 + u orders exactly like
            // (v, u) for u < v, and the field compare cannot overflow.
            .then_with(|| (a.v, a.u).cmp(&(b.v, b.u)))
    });

    let mut adj: Vec<Vec<AdjEntry>> = vec![Vec::new(); n];
    for (idx, e) in edges.iter().enumerate() {
        adj[e.u].push((e.v, e.value, idx));
        adj[e.v].push((e.u, e.value, idx));
    }
    for list in &mut adj {
        list.sort_unstable_by_key(|&(x, _, _)| x);
    }

    Ok(Prepared {
        edges,
        adj,
        run: Run {
            n,
            terminal,
            threshold,
        },
    })
}

/// Close a run: keep the live edges, build the reduced matrix, and record
/// the removals in a certificate. `stats` holds the counters the execution
/// kept, and this step adds the totals that follow from `steps`.
fn finish(
    run: Run,
    execution: Execution,
    edges: &[EdgeRec],
    steps: Vec<RemovalStep>,
    mut stats: CollapseStats,
    timings: CollapseTimings,
) -> Result<CollapsedRips> {
    let input_edges = edges.len();
    stats.removed_edges = steps.len();
    stats.output_edges = input_edges - steps.len();
    if !matches!(execution, Execution::Ordered) {
        // The serial and rounds executions test exactly the logical
        // sequence, so the physical count is the logical count. The
        // ordered execution counts its logical trace as it retires.
        stats.logical_tests = stats.edge_tests;
    }

    let survivors: Vec<(usize, usize, f64)> = edges
        .iter()
        .filter(|e| e.alive)
        .map(|e| (e.u, e.v, e.value))
        .collect();
    let matrix = SparseDistanceMatrix::from_triplets(run.n, &survivors)?;
    let (algorithm_version, objective, completeness, work_limit, work_used) = match execution {
        Execution::Serial | Execution::Ordered => {
            (1, None, CollapseCompleteness::CompleteFixedPoint, None, 0)
        }
        Execution::Snapshot => (2, None, CollapseCompleteness::CompleteFixedPoint, None, 0),
        Execution::Adaptive {
            objective,
            completeness,
            work_limit,
            work_used,
        } => (3, Some(objective), completeness, work_limit, work_used),
    };
    let certificate = CollapseCertificate {
        algorithm_version,
        objective,
        completeness,
        work_limit,
        work_used,
        vertex_count: run.n,
        requested_threshold: run.threshold,
        terminal_level: run.terminal,
        input_edge_count: input_edges,
        output_edge_count: stats.output_edges,
        steps,
    };
    Ok(CollapsedRips {
        matrix,
        certificate,
        stats,
        timings,
    })
}

/// A rayon pool with `threads` workers, owned by one call.
fn build_pool(threads: usize) -> Result<rayon::ThreadPool> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|e| Error::Io(format!("thread pool: {e}")))
}

/// Tombstone both adjacency entries of a removed edge. The entries stay in
/// place so binary search and the sorted merge keep working.
fn tombstone(adj: &mut [Vec<AdjEntry>], u: usize, v: usize) {
    for (a, b) in [(u, v), (v, u)] {
        if let Ok(pos) = adj[a].binary_search_by(|probe| probe.0.cmp(&b)) {
            adj[a][pos].1 = f64::INFINITY;
        }
    }
}

/// Reused buffers for the edge test and the dirty marking.
#[derive(Default)]
struct Scratch {
    /// C as `(vertex, b)`, ascending vertex order.
    cands: Vec<(usize, f64)>,
    /// `(b, position)` pairs sorted by `b`, then position: the entrant
    /// runs, with the sort key held directly in the element.
    by_b: Vec<(f64, u32)>,
    /// `f(apex, cands[q])` for the current apex; 0 at the apex itself.
    apex_row: Vec<f64>,
    /// The affected-vertex set S during dirty marking.
    marks: Vec<usize>,
}

/// Above this |S|, fine-grained dirty marking costs more than a plain
/// retest of every live edge in the next pass, so the caller falls back.
const MARK_LIMIT: usize = 64;

/// `f(w, x)` for one pair by binary search in `w`'s adjacency list.
fn pair_value(list: &[AdjEntry], x: usize) -> f64 {
    match list.binary_search_by(|probe| probe.0.cmp(&x)) {
        Ok(pos) => list[pos].1,
        Err(_) => f64::INFINITY,
    }
}

/// True when the candidate at `pos` dominates at level `t`: `f(w, x) <= t`
/// for every level member `x`. `members` holds the candidate positions with
/// `b <= t`. A small member set probes by binary search; a large one runs a
/// single forward scan of `adj[w]` against the vertex-sorted candidate
/// list. Both leave on the first violation.
fn dominates(
    adj: &[Vec<AdjEntry>],
    cands: &[(usize, f64)],
    members: &[(f64, u32)],
    pos: usize,
    t: f64,
) -> bool {
    let list = &adj[cands[pos].0];
    if members.len() * 16 < list.len() {
        return members.iter().all(|&(_, q)| {
            let q = q as usize;
            q == pos || pair_value(list, cands[q].0) <= t
        });
    }
    let mut i = 0;
    for (q, &(x, b)) in cands.iter().enumerate() {
        if q == pos || b > t {
            continue;
        }
        while i < list.len() && list[i].0 < x {
            i += 1;
        }
        if i >= list.len() || list[i].0 != x || list[i].1 > t {
            return false;
        }
    }
    true
}

/// Fill `row` with `f(w, x)` over the candidate list, where `w` is the
/// candidate at `pos`: +inf for non-neighbors, 0 at `pos` itself. A short
/// candidate list probes by binary search; a long one merges.
fn fill_row(adj: &[Vec<AdjEntry>], cands: &[(usize, f64)], pos: usize, row: &mut Vec<f64>) {
    row.clear();
    row.resize(cands.len(), f64::INFINITY);
    let list = &adj[cands[pos].0];
    if cands.len() * 16 < list.len() {
        for (q, &(x, _)) in cands.iter().enumerate() {
            row[q] = pair_value(list, x);
        }
    } else {
        let (mut i, mut q) = (0, 0);
        while i < list.len() && q < cands.len() {
            let (x, d, _) = list[i];
            match x.cmp(&cands[q].0) {
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => q += 1,
                std::cmp::Ordering::Equal => {
                    row[q] = d;
                    i += 1;
                    q += 1;
                }
            }
        }
    }
    row[pos] = 0.0;
}

/// Test one live edge `{u, v}` with value `a` against the current graph.
/// Return the witness segments when the edge is removable, `None` when it
/// is not. `s.cands` is left holding `C` so the caller can read `|C|`.
///
/// The sweep walks the entrant runs of the b-sorted candidate order. A kept
/// apex is rechecked against the entrants only: earlier members already
/// satisfied `f(apex, x) <= t'` at a smaller `t'`. A rescan streams one
/// candidate row at a time in increasing vertex order and keeps the first
/// dominating row as the new apex row.
fn test_edge(
    adj: &[Vec<AdjEntry>],
    u: usize,
    v: usize,
    a: f64,
    terminal: f64,
    s: &mut Scratch,
) -> Option<Vec<(f64, usize)>> {
    collect_candidates(adj, u, v, a, terminal, &mut s.cands);
    if s.cands.is_empty() {
        return None;
    }
    order_candidates(&s.cands, &mut s.by_b);
    if s.by_b[0].0 > a {
        return None;
    }
    witness_segments(adj, &s.cands, &s.by_b, &mut s.apex_row)
}

fn collect_candidates(
    adj: &[Vec<AdjEntry>],
    u: usize,
    v: usize,
    edge_value: f64,
    terminal: f64,
    candidates: &mut Vec<(usize, f64)>,
) {
    candidates.clear();
    let (lu, lv) = (&adj[u], &adj[v]);
    let (mut i, mut j) = (0, 0);
    while i < lu.len() && j < lv.len() {
        let (x, du, _) = lu[i];
        let (y, dv, _) = lv[j];
        match x.cmp(&y) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                if du.is_finite() && dv.is_finite() {
                    let b = edge_value.max(du).max(dv);
                    if b <= terminal {
                        candidates.push((x, b));
                    }
                }
                i += 1;
                j += 1;
            }
        }
    }
}

fn order_candidates(candidates: &[(usize, f64)], by_birth: &mut Vec<(f64, u32)>) {
    by_birth.clear();
    by_birth.extend(
        candidates
            .iter()
            .enumerate()
            .map(|(position, &(_, birth))| (birth, position as u32)),
    );
    by_birth.sort_unstable_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
}

fn witness_segments(
    adj: &[Vec<AdjEntry>],
    candidates: &[(usize, f64)],
    by_birth: &[(f64, u32)],
    apex_row: &mut Vec<f64>,
) -> Option<Vec<(f64, usize)>> {
    let mut segments: Vec<(f64, usize)> = Vec::new();
    let mut run = 0usize;
    let mut have_apex = false;
    while run < candidates.len() {
        let t = by_birth[run].0;
        let mut run_end = run;
        while run_end < candidates.len() && by_birth[run_end].0 == t {
            run_end += 1;
        }
        let kept = have_apex
            && by_birth[run..run_end]
                .iter()
                .all(|&(_, position)| apex_row[position as usize] <= t);
        if !kept {
            let position = first_dominating_candidate(adj, candidates, &by_birth[..run_end], t)?;
            fill_row(adj, candidates, position, apex_row);
            segments.push((t, candidates[position].0));
            have_apex = true;
        }
        run = run_end;
    }
    Some(segments)
}

fn first_dominating_candidate(
    adj: &[Vec<AdjEntry>],
    candidates: &[(usize, f64)],
    members: &[(f64, u32)],
    level: f64,
) -> Option<usize> {
    (0..candidates.len()).find(|&position| {
        candidates[position].1 <= level && dominates(adj, candidates, members, position, level)
    })
}

/// Run `mark` on the schedule index of every live edge of the subgraph
/// induced by S = N[u] intersect N[v] (closed neighborhoods) in the
/// current graph. With `limit` set, the walk bails and returns false
/// before it visits anything when the neighborhood is larger than the
/// limit allows; `s.marks` is unusable after a bail. Without a limit the
/// walk is exact, which conflict blocking in the rounds schedule needs.
fn for_each_induced_edge(
    adj: &[Vec<AdjEntry>],
    u: usize,
    v: usize,
    s: &mut Scratch,
    limit: Option<usize>,
    mut mark: impl FnMut(usize),
) -> bool {
    let (lu, lv) = (&adj[u], &adj[v]);
    if limit.is_some_and(|limit| lu.len().min(lv.len()) > 2 * limit) {
        return false;
    }
    collect_closed_common(lu, lv, u, v, &mut s.marks);
    if limit.is_some_and(|limit| s.marks.len() > limit) {
        return false;
    }
    for (position, &vertex) in s.marks.iter().enumerate() {
        mark_induced_edges(adj, &s.marks, position, vertex, &mut mark);
    }
    true
}

fn collect_closed_common(
    left: &[AdjEntry],
    right: &[AdjEntry],
    u: usize,
    v: usize,
    vertices: &mut Vec<usize>,
) {
    vertices.clear();
    let (mut i, mut j) = (0, 0);
    while i < left.len() && j < right.len() {
        let (x, du, _) = left[i];
        let (y, dv, _) = right[j];
        match x.cmp(&y) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                if du.is_finite() && dv.is_finite() {
                    vertices.push(x);
                }
                i += 1;
                j += 1;
            }
        }
    }
    vertices.push(u);
    vertices.push(v);
    vertices.sort_unstable();
}

fn mark_induced_edges(
    adj: &[Vec<AdjEntry>],
    vertices: &[usize],
    position: usize,
    vertex: usize,
    mark: &mut impl FnMut(usize),
) {
    let list = &adj[vertex];
    if vertices.len() * 16 < list.len() {
        mark_induced_edges_by_probe(list, &vertices[position + 1..], mark);
    } else {
        mark_induced_edges_by_merge(list, vertices, vertex, mark);
    }
}

fn mark_induced_edges_by_probe(
    list: &[AdjEntry],
    vertices: &[usize],
    mark: &mut impl FnMut(usize),
) {
    for &other in vertices {
        if let Ok(position) = list.binary_search_by(|probe| probe.0.cmp(&other)) {
            let (_, distance, index) = list[position];
            if distance.is_finite() {
                mark(index);
            }
        }
    }
}

fn mark_induced_edges_by_merge(
    list: &[AdjEntry],
    vertices: &[usize],
    vertex: usize,
    mark: &mut impl FnMut(usize),
) {
    let (mut i, mut q) = (0, 0);
    while i < list.len() && q < vertices.len() {
        let (neighbor, distance, index) = list[i];
        match neighbor.cmp(&vertices[q]) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => q += 1,
            std::cmp::Ordering::Equal => {
                if neighbor > vertex && distance.is_finite() {
                    mark(index);
                }
                i += 1;
                q += 1;
            }
        }
    }
}

/// Mark every live edge whose test could change after `{u, v}` goes away.
/// A verdict depends only on edges inside `{u, v} union C`, and every such
/// vertex lies in both closed neighborhoods of the removed pair, so marking
/// all live edges with both endpoints in S = N[u] intersect N[v] is a
/// conservative cover. Runs before the tombstone so S still sees the edge.
/// Returns false without marking when S exceeds [`MARK_LIMIT`]; the caller
/// then retests every live edge next pass, which is sound (a superset of
/// the dirty set) and cheaper on dense neighborhoods.
fn mark_dirty(
    adj: &[Vec<AdjEntry>],
    dirty: &mut [bool],
    u: usize,
    v: usize,
    s: &mut Scratch,
) -> bool {
    for_each_induced_edge(adj, u, v, s, Some(MARK_LIMIT), |idx| dirty[idx] = true)
}

/// The serial version 1 collapse for the pipeline.
pub(crate) fn collapse_serial_in<D: Distances>(
    dist: &D,
    threshold: Option<f64>,
) -> Result<CollapsedRips> {
    collapse_impl(dist, threshold)
}

fn collapse_impl<D: Distances>(dist: &D, threshold: Option<f64>) -> Result<CollapsedRips> {
    let Prepared {
        mut edges,
        mut adj,
        run,
    } = prepare(dist, threshold)?;

    let mut stats = CollapseStats::new(edges.len());
    let mut steps: Vec<RemovalStep> = Vec::new();
    let mut scratch = Scratch::default();
    // A failed verdict can change only when an edge inside the test's own
    // neighborhood goes away, so later passes retest only edges marked by
    // `mark_dirty`, or every live edge again after a removal whose
    // neighborhood was too large to mark finely. Both are supersets of the
    // edges whose verdicts could have changed, so the removal sequence, and
    // with it the certificate, is identical to retesting everything.
    let mut dirty: Vec<bool> = vec![false; edges.len()];
    let mut test_all = true;
    loop {
        stats.epochs += 1;
        let mut removed_any = false;
        let mut test_all_next = false;
        for idx in 0..edges.len() {
            if !edges[idx].alive || !(test_all || dirty[idx]) {
                continue;
            }
            dirty[idx] = false;
            let (u, v, value) = (edges[idx].u, edges[idx].v, edges[idx].value);
            stats.edge_tests += 1;
            let witnesses = test_edge(&adj, u, v, value, run.terminal, &mut scratch);
            stats.max_common_neighborhood = stats.max_common_neighborhood.max(scratch.cands.len());
            if let Some(witnesses) = witnesses {
                edges[idx].alive = false;
                if !mark_dirty(&adj, &mut dirty, u, v, &mut scratch) {
                    test_all_next = true;
                }
                tombstone(&mut adj, u, v);
                stats.witness_segments += witnesses.len();
                steps.push(RemovalStep {
                    u,
                    v,
                    value,
                    position: SchedulePosition::Pass(stats.epochs),
                    witnesses,
                });
                removed_any = true;
            }
        }
        if !removed_any {
            break;
        }
        test_all = test_all_next;
    }

    finish(
        run,
        Execution::Serial,
        &edges,
        steps,
        stats,
        CollapseTimings::default(),
    )
}

/// Shared threshold validation, matching the solver's rule.
fn validate_threshold(threshold: Option<f64>) -> Result<()> {
    if let Some(t) = threshold {
        if t.is_nan() || t < 0.0 {
            return Err(Error::InvalidInput(format!(
                "threshold must be non-negative, got {t}"
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges_of(m: &SparseDistanceMatrix) -> Vec<(usize, usize, f64)> {
        m.edges().collect()
    }

    fn check_invariants(r: &CollapsedRips) {
        let c = &r.certificate;
        assert_eq!(c.algorithm_version(), 1);
        assert_eq!(c.objective(), None);
        assert_eq!(c.completeness(), CollapseCompleteness::CompleteFixedPoint);
        assert_eq!(c.work_limit(), None);
        assert_eq!(c.work_used(), 0);
        assert_eq!(
            c.input_edge_count(),
            c.output_edge_count() + c.steps().len()
        );
        assert_eq!(r.stats.input_edges, c.input_edge_count());
        assert_eq!(r.stats.output_edges, c.output_edge_count());
        assert_eq!(r.stats.removed_edges, c.steps().len());
        assert_eq!(r.stats.output_edges, r.matrix.num_edges());
        assert_eq!(
            r.stats.witness_segments,
            c.steps().iter().map(|s| s.witnesses().len()).sum::<usize>()
        );
        assert!(r.stats.epochs >= 1);
        for s in c.steps() {
            assert!(s.edge().0 < s.edge().1);
            assert!(s.position().number() >= 1);
            assert!(
                s.position().number() < r.stats.epochs,
                "final pass removes nothing"
            );
            assert!(!s.witnesses().is_empty());
            assert_eq!(s.witnesses()[0].0, s.value());
            for w in s.witnesses().windows(2) {
                assert!(w[0].0 < w[1].0);
            }
            assert!(s.witnesses().iter().all(|&(_, w)| w < c.vertex_count()));
        }
    }

    // Complete graph on 6 vertices with ties, a zero edge, and two entries
    // above the enclosing radius.
    fn tie_heavy_condensed() -> Vec<f64> {
        vec![
            0.0, // 1-0
            1.0, 1.0, // 2-*
            2.0, 2.0, 1.0, // 3-*
            3.0, 1.0, 2.0, 2.0, // 4-*
            1.0, 3.0, 2.0, 1.0, 2.0, // 5-*
        ]
    }

    #[test]
    fn triangle_collapses_the_first_scheduled_edge() {
        let d = DistanceMatrix::from_condensed(vec![1.0, 1.0, 1.0]).unwrap();
        let r = collapse_dense(&d, None).unwrap();
        check_invariants(&r);
        let steps = r.certificate.steps();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].edge(), (0, 1));
        assert_eq!(steps[0].value(), 1.0);
        assert_eq!(steps[0].position().number(), 1);
        assert_eq!(steps[0].witnesses(), &[(1.0, 2)]);
        assert_eq!(edges_of(&r.matrix), vec![(0, 2, 1.0), (1, 2, 1.0)]);
        assert_eq!(r.certificate.terminal_level(), 1.0);
        assert_eq!(r.stats.epochs, 2);
    }

    #[test]
    fn chordless_four_cycle_survives() {
        let m = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let r = collapse_sparse(&m, None).unwrap();
        check_invariants(&r);
        assert!(r.certificate.steps().is_empty());
        assert_eq!(r.matrix.num_edges(), 4);
        assert_eq!(r.stats.epochs, 1);
        assert_eq!(r.stats.edge_tests, 4);
    }

    // Schedule order on unit K4: (0,1), (0,2), (1,2), then the three edges
    // at vertex 3. (0,1) goes first with apex 2 (lowest common neighbor);
    // after that removal the only common neighbor of each remaining pair in
    // {0,1,2} is 3, so (0,2) and (1,2) fall with apex 3 and the star at 3
    // has no removable edge.
    #[test]
    fn k4_collapses_to_a_spanning_star() {
        let d = DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap();
        let r = collapse_dense(&d, None).unwrap();
        check_invariants(&r);
        let removed: Vec<_> = r.certificate.steps().iter().map(|s| s.edge()).collect();
        assert_eq!(removed, vec![(0, 1), (0, 2), (1, 2)]);
        let witnesses: Vec<_> = r
            .certificate
            .steps()
            .iter()
            .map(|s| s.witnesses().to_vec())
            .collect();
        assert_eq!(
            witnesses,
            vec![vec![(1.0, 2)], vec![(1.0, 3)], vec![(1.0, 3)]]
        );
        assert!(
            r.certificate
                .steps()
                .iter()
                .all(|s| s.position().number() == 1)
        );
        assert_eq!(
            edges_of(&r.matrix),
            vec![(0, 3, 1.0), (1, 3, 1.0), (2, 3, 1.0)]
        );
        assert_eq!(r.stats.epochs, 2);
    }

    #[test]
    fn isolated_edge_survives() {
        let m = SparseDistanceMatrix::from_triplets(
            5,
            &[(0, 1, 1.0), (0, 2, 1.0), (1, 2, 1.0), (3, 4, 1.0)],
        )
        .unwrap();
        let r = collapse_sparse(&m, None).unwrap();
        check_invariants(&r);
        let removed: Vec<_> = r.certificate.steps().iter().map(|s| s.edge()).collect();
        assert_eq!(removed, vec![(0, 1)]);
        assert!(edges_of(&r.matrix).contains(&(3, 4, 1.0)));
    }

    #[test]
    fn threshold_drops_edges_before_collapse() {
        let d = DistanceMatrix::from_condensed(vec![1.0, 1.0, 5.0]).unwrap();
        let r = collapse_dense(&d, Some(2.0)).unwrap();
        check_invariants(&r);
        assert_eq!(r.certificate.requested_threshold(), Some(2.0));
        assert_eq!(r.certificate.terminal_level(), 2.0);
        assert_eq!(r.certificate.input_edge_count(), 2);
        assert!(r.certificate.steps().is_empty());
        assert_eq!(edges_of(&r.matrix), vec![(0, 1, 1.0), (0, 2, 1.0)]);
        assert_eq!(r.stats.epochs, 1);
    }

    #[test]
    fn infinite_dense_entries_are_absent_edges() {
        let d = DistanceMatrix::from_condensed(vec![1.0, 1.0, f64::INFINITY]).unwrap();
        let r = collapse_dense(&d, Some(f64::INFINITY)).unwrap();
        check_invariants(&r);
        assert_eq!(r.certificate.input_edge_count(), 2);
        assert_eq!(r.certificate.terminal_level(), 1.0);
        assert!(r.certificate.steps().is_empty());
    }

    #[test]
    fn reruns_are_identical() {
        let d = DistanceMatrix::from_condensed(tie_heavy_condensed()).unwrap();
        let a = collapse_dense(&d, None).unwrap();
        let b = collapse_dense(&d, None).unwrap();
        assert_eq!(a.certificate, b.certificate);
        assert_eq!(a.stats, b.stats);
        assert_eq!(edges_of(&a.matrix), edges_of(&b.matrix));
        assert!(a.stats.removed_edges > 0);
        check_invariants(&a);
    }

    #[test]
    fn dense_and_sparse_agree() {
        let condensed = tie_heavy_condensed();
        let d = DistanceMatrix::from_condensed(condensed.clone()).unwrap();
        let mut triplets = Vec::new();
        let mut k = 0;
        for i in 1..6 {
            for j in 0..i {
                triplets.push((i, j, condensed[k]));
                k += 1;
            }
        }
        let s = SparseDistanceMatrix::from_triplets(6, &triplets).unwrap();
        let rd = collapse_dense(&d, Some(2.0)).unwrap();
        let rs = collapse_sparse(&s, Some(2.0)).unwrap();
        check_invariants(&rd);
        assert_eq!(rd.certificate, rs.certificate);
        assert_eq!(rd.stats, rs.stats);
        assert_eq!(edges_of(&rd.matrix), edges_of(&rs.matrix));
        assert!(rd.stats.removed_edges > 0);
    }

    // Exact counters for unit K4: pass 1 tests 6 edges and removes 3, pass
    // 2 tests the 3 survivors, every removal has one segment, and the
    // largest C is the pair {2, 3} seen by edge (0,1).
    #[test]
    fn stats_match_the_certificate() {
        let d = DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap();
        let r = collapse_dense(&d, None).unwrap();
        check_invariants(&r);
        assert_eq!(r.stats.input_edges, 6);
        assert_eq!(r.stats.output_edges, 3);
        assert_eq!(r.stats.removed_edges, 3);
        assert_eq!(r.stats.epochs, 2);
        // Every edge is tested once in pass 1. The three survivors are last
        // dirtied before their own pass-1 tests, so pass 2 retests nothing.
        assert_eq!(r.stats.edge_tests, 6);
        assert_eq!(r.stats.witness_segments, 3);
        assert_eq!(r.stats.max_common_neighborhood, 2);
    }

    #[test]
    fn empty_inputs_yield_empty_certificates() {
        let d0 = DistanceMatrix::from_points(&[]).unwrap();
        let d1 = DistanceMatrix::from_condensed(vec![]).unwrap();
        let s0 = SparseDistanceMatrix::from_triplets(0, &[]).unwrap();
        for r in [
            collapse_dense(&d0, None).unwrap(),
            collapse_dense(&d1, None).unwrap(),
            collapse_sparse(&s0, None).unwrap(),
        ] {
            check_invariants(&r);
            assert_eq!(r.certificate.input_edge_count(), 0);
            assert_eq!(r.certificate.terminal_level(), 0.0);
            assert!(r.certificate.steps().is_empty());
            assert_eq!(r.stats.epochs, 1);
            assert_eq!(r.stats.edge_tests, 0);
        }
    }

    #[test]
    fn invalid_thresholds_are_rejected() {
        let d = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
        assert!(collapse_dense(&d, Some(-1.0)).is_err());
        assert!(collapse_dense(&d, Some(f64::NAN)).is_err());
    }
}
