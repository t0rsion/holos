//! Filtered edge collapse for flag filtrations.
//!
//! The collapse removes edges that are dominated at every scale from their
//! birth to the terminal level (the filtration-wide multi-witness criterion
//! of Boissonnat and Pritam). The flag filtration of the reduced graph has
//! the same persistence diagram as the input, in every dimension. Each
//! removal is recorded in a replayable [`CollapseCertificate`] that the
//! independent checker in [`verify`] can validate.
//!
//! The collapse is serial. It is deterministic given the vertex labeling.
//! The reduced graph is not canonical: a relabeling can change which edges
//! survive, but never the barcode.

pub mod verify;

use crate::distances::Distances;
use crate::{DistanceMatrix, Error, Result, SparseDistanceMatrix};

/// One removed edge: endpoints, original value, pass number, and the
/// piecewise witness function that certifies the removal.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalStep {
    u: usize,
    v: usize,
    value: f64,
    pass: usize,
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

    /// 1-based pass in which the edge was removed.
    pub fn pass(&self) -> usize {
        self.pass
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CollapseStats {
    /// Edges in the thresholded input.
    pub input_edges: usize,
    /// Edges that survived.
    pub output_edges: usize,
    /// Edges removed.
    pub removed_edges: usize,
    /// Passes over the edge list, including the final pass that removes
    /// nothing.
    pub passes: usize,
    /// Predicate evaluations across all passes.
    pub edge_tests: usize,
    /// Witness segments recorded across all removal steps.
    pub witness_segments: usize,
    /// Largest common-neighborhood size seen by the predicate.
    pub max_common_neighborhood: usize,
}

/// A collapsed filtration: the reduced graph, the certificate that the
/// reduction preserves the diagram, and run counters.
///
/// The matrix drops into [`crate::rips_persistence_sparse`] directly. One
/// collapse can serve many downstream runs: the reduction is independent of
/// modulus, homology dimension, optimization toggles, and thread count.
#[derive(Debug, Clone)]
pub struct CollapsedRips {
    /// The reduced graph. Surviving edge values are the input values, bit
    /// for bit.
    pub matrix: SparseDistanceMatrix,
    /// Replayable proof of every removal.
    pub certificate: CollapseCertificate,
    /// Run counters.
    pub stats: CollapseStats,
}

/// Collapse a dense distance matrix.
///
/// `threshold` follows the engine's rule: `None` means the enclosing
/// radius. Edges above the resolved threshold are dropped before the
/// collapse and are not part of the certified input.
pub fn collapse_dense(dist: &DistanceMatrix, threshold: Option<f64>) -> Result<CollapsedRips> {
    collapse_impl(dist, threshold)
}

/// Collapse a sparse distance matrix.
///
/// `threshold` follows the engine's rule: `None` keeps every listed edge.
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
    // C by sorted merge of the two adjacency lists. Tombstones read as
    // +inf and drop out through the finiteness checks. Neither list holds
    // its own vertex, so u and v never enter the intersection.
    s.cands.clear();
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
                    let b = a.max(du).max(dv);
                    if b <= terminal {
                        s.cands.push((x, b));
                    }
                }
                i += 1;
                j += 1;
            }
        }
    }
    let cands = &s.cands;
    let k = cands.len();
    if k == 0 {
        return None;
    }

    s.by_b.clear();
    s.by_b
        .extend(cands.iter().enumerate().map(|(p, &(_, b))| (b, p as u32)));
    s.by_b
        .sort_unstable_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    // The critical values are the distinct b values plus `a` itself. At
    // t = a the candidate set is `{x : b(x) = a}`; when the smallest b
    // exceeds `a` that set is empty and no witness exists at the birth.
    if s.by_b[0].0 > a {
        return None;
    }

    let mut segments: Vec<(f64, usize)> = Vec::new();
    let mut apex: Option<usize> = None;
    let mut run = 0usize;
    while run < k {
        let t = s.by_b[run].0;
        let mut run_end = run;
        while run_end < k && s.by_b[run_end].0 == t {
            run_end += 1;
        }
        let kept = apex.is_some_and(|_| {
            s.by_b[run..run_end]
                .iter()
                .all(|&(_, p)| s.apex_row[p as usize] <= t)
        });
        if !kept {
            let mut found = None;
            for p in 0..k {
                if cands[p].1 > t {
                    continue;
                }
                if dominates(adj, cands, &s.by_b[..run_end], p, t) {
                    found = Some(p);
                    break;
                }
            }
            match found {
                Some(p) => {
                    fill_row(adj, cands, p, &mut s.apex_row);
                    segments.push((t, cands[p].0));
                    apex = Some(p);
                }
                None => return None,
            }
        }
        run = run_end;
    }
    Some(segments)
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
    // The whole point of fine marking is to beat a plain retest, so its own
    // cost must stay near-constant. Long adjacency lists mean a dense
    // neighborhood where a retest is cheap per edge anyway: bail before
    // walking anything.
    let (lu, lv) = (&adj[u], &adj[v]);
    if lu.len().min(lv.len()) > 2 * MARK_LIMIT {
        return false;
    }
    s.marks.clear();
    let (mut i, mut j) = (0, 0);
    while i < lu.len() && j < lv.len() {
        let (x, du, _) = lu[i];
        let (y, dv, _) = lv[j];
        match x.cmp(&y) {
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
            std::cmp::Ordering::Equal => {
                if du.is_finite() && dv.is_finite() {
                    s.marks.push(x);
                }
                i += 1;
                j += 1;
            }
        }
    }
    s.marks.push(u);
    s.marks.push(v);
    if s.marks.len() > MARK_LIMIT {
        return false;
    }
    s.marks.sort_unstable();
    for (a, &p) in s.marks.iter().enumerate() {
        let list = &adj[p];
        // A long list gets probed per pair; a short one merges.
        if s.marks.len() * 16 < list.len() {
            for &q in &s.marks[a + 1..] {
                if let Ok(pos) = list.binary_search_by(|probe| probe.0.cmp(&q)) {
                    let (_, d, idx) = list[pos];
                    if d.is_finite() {
                        dirty[idx] = true;
                    }
                }
            }
        } else {
            let (mut i, mut q) = (0, 0);
            while i < list.len() && q < s.marks.len() {
                let (x, d, idx) = list[i];
                match x.cmp(&s.marks[q]) {
                    std::cmp::Ordering::Less => i += 1,
                    std::cmp::Ordering::Greater => q += 1,
                    std::cmp::Ordering::Equal => {
                        if x > p && d.is_finite() {
                            dirty[idx] = true;
                        }
                        i += 1;
                        q += 1;
                    }
                }
            }
        }
    }
    true
}

fn collapse_impl<D: Distances>(dist: &D, threshold: Option<f64>) -> Result<CollapsedRips> {
    validate_threshold(threshold)?;
    let n = dist.len();
    let resolved = threshold.unwrap_or_else(|| dist.default_threshold());

    let mut edges: Vec<EdgeRec> = Vec::new();
    dist.for_each_edge(&mut |i, j, d| {
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

    let input_edges = edges.len();
    let mut stats = CollapseStats {
        input_edges,
        output_edges: input_edges,
        removed_edges: 0,
        passes: 0,
        edge_tests: 0,
        witness_segments: 0,
        max_common_neighborhood: 0,
    };
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
        stats.passes += 1;
        let mut removed_any = false;
        let mut test_all_next = false;
        for idx in 0..edges.len() {
            if !edges[idx].alive || !(test_all || dirty[idx]) {
                continue;
            }
            dirty[idx] = false;
            let (u, v, value) = (edges[idx].u, edges[idx].v, edges[idx].value);
            stats.edge_tests += 1;
            let witnesses = test_edge(&adj, u, v, value, terminal, &mut scratch);
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
                    pass: stats.passes,
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
    stats.removed_edges = steps.len();
    stats.output_edges = input_edges - steps.len();

    let survivors: Vec<(usize, usize, f64)> = edges
        .iter()
        .filter(|e| e.alive)
        .map(|e| (e.u, e.v, e.value))
        .collect();
    let matrix = SparseDistanceMatrix::from_triplets(n, &survivors)?;
    let certificate = CollapseCertificate {
        algorithm_version: 1,
        vertex_count: n,
        requested_threshold: threshold,
        terminal_level: terminal,
        input_edge_count: input_edges,
        output_edge_count: stats.output_edges,
        steps,
    };
    Ok(CollapsedRips {
        matrix,
        certificate,
        stats,
    })
}

/// Shared threshold validation, matching the solver's rule.
pub(crate) fn validate_threshold(threshold: Option<f64>) -> Result<()> {
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
        assert!(r.stats.passes >= 1);
        for s in c.steps() {
            assert!(s.edge().0 < s.edge().1);
            assert!(s.pass() >= 1);
            assert!(s.pass() < r.stats.passes, "final pass removes nothing");
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
        assert_eq!(steps[0].pass(), 1);
        assert_eq!(steps[0].witnesses(), &[(1.0, 2)]);
        assert_eq!(edges_of(&r.matrix), vec![(0, 2, 1.0), (1, 2, 1.0)]);
        assert_eq!(r.certificate.terminal_level(), 1.0);
        assert_eq!(r.stats.passes, 2);
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
        assert_eq!(r.stats.passes, 1);
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
        assert!(r.certificate.steps().iter().all(|s| s.pass() == 1));
        assert_eq!(
            edges_of(&r.matrix),
            vec![(0, 3, 1.0), (1, 3, 1.0), (2, 3, 1.0)]
        );
        assert_eq!(r.stats.passes, 2);
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
        assert_eq!(r.stats.passes, 1);
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
        assert_eq!(r.stats.passes, 2);
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
            assert_eq!(r.stats.passes, 1);
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
