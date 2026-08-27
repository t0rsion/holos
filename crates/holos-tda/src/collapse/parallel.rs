//! The rounds schedule: frozen-graph rounds with deterministic greedy
//! batches, written as algorithm version 2 certificates.
//!
//! Each round tests the live edges against a frozen copy of the graph,
//! orders the removable ones by the frozen priority, selects a greedy
//! maximal set of pairwise non-conflicting edges, and deletes the batch.
//! Two edges conflict when one lies inside the subgraph induced by the
//! other's closed common neighborhood. The matrix and certificate are
//! identical at every worker count, including one, field for field.
//!
//! The rounds graph is not the serial graph: the schedules differ, and
//! neither output is canonical. Both preserve the barcode exactly.

use rayon::prelude::*;

use super::{
    AdjEntry, CollapseStats, CollapseTimings, CollapsedRips, EdgeRec, Execution, Prepared,
    RemovalStep, Run, SchedulePosition, Scratch, build_pool, finish, for_each_induced_edge,
    mark_dirty, prepare, test_edge, tombstone,
};
use crate::distances::Distances;
use crate::{DistanceMatrix, Result, SparseDistanceMatrix};

/// Collapse a dense distance matrix with the rounds schedule.
///
/// `threshold` follows the engine's rule: `None` means the enclosing
/// radius. `threads` of 0 or 1 run one worker; the result does not depend
/// on the worker count. A standalone call owns its thread pool for the
/// duration.
pub fn collapse_dense_rounds_parallel(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> Result<CollapsedRips> {
    collapse_v2_owned(dist, threshold, threads)
}

/// Collapse a sparse distance matrix with the rounds schedule.
///
/// Same contract as [`collapse_dense_rounds_parallel`]; `None` keeps every
/// listed edge.
pub fn collapse_sparse_rounds_parallel(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> Result<CollapsedRips> {
    collapse_v2_owned(dist, threshold, threads)
}

/// The piecewise witness segments of one removable edge.
type Witnesses = Vec<(f64, usize)>;

/// Build an owned pool for the call (none for one worker) and run the
/// rounds schedule on it.
fn collapse_v2_owned<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    threads: usize,
) -> Result<CollapsedRips> {
    let pool = if threads.max(1) > 1 {
        Some(build_pool(threads)?)
    } else {
        None
    };
    collapse_rounds_in(dist, threshold, pool.as_ref())
}

/// The rounds schedule on a caller-provided pool (`None` runs one
/// worker). The public wrappers build and own one; the pipeline shares
/// its run-wide pool through here when the rounds schedule is selected.
pub(crate) fn collapse_rounds_in<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    pool: Option<&rayon::ThreadPool>,
) -> Result<CollapsedRips> {
    SnapshotExecution::new(prepare(dist, threshold)?).run(pool)
}

struct SnapshotExecution {
    edges: Vec<EdgeRec>,
    adj: Vec<Vec<AdjEntry>>,
    run: Run,
    stats: CollapseStats,
    steps: Vec<RemovalStep>,
    scratch: Scratch,
    dirty: Vec<bool>,
    blocked: Vec<usize>,
    test_all: bool,
}

impl SnapshotExecution {
    fn new(prepared: Prepared) -> Self {
        let count = prepared.edges.len();
        Self {
            edges: prepared.edges,
            adj: prepared.adj,
            run: prepared.run,
            stats: CollapseStats::new(count),
            steps: Vec::new(),
            scratch: Scratch::default(),
            dirty: vec![false; count],
            blocked: vec![0; count],
            test_all: true,
        }
    }

    fn run(mut self, pool: Option<&rayon::ThreadPool>) -> Result<CollapsedRips> {
        while self.run_round(pool) {}
        finish(
            self.run,
            Execution::Snapshot,
            &self.edges,
            self.steps,
            self.stats,
            CollapseTimings::default(),
        )
    }

    fn run_round(&mut self, pool: Option<&rayon::ThreadPool>) -> bool {
        self.stats.epochs += 1;
        let due = self.due_edges();
        let mut results = self.test_due(pool, &due);
        let selected = self.select(&due, &results);
        if selected.is_empty() {
            return false;
        }
        self.commit(&due, &selected, &mut results);
        true
    }

    fn due_edges(&mut self) -> Vec<usize> {
        let mut due = Vec::new();
        for index in 0..self.edges.len() {
            if self.edges[index].alive && (self.test_all || self.dirty[index]) {
                self.dirty[index] = false;
                due.push(index);
            }
        }
        due
    }

    fn test_due(
        &mut self,
        pool: Option<&rayon::ThreadPool>,
        due: &[usize],
    ) -> Vec<(Option<Witnesses>, usize)> {
        self.stats.edge_tests += due.len();
        let results = match pool {
            Some(pool) => pool.install(|| {
                due.par_iter()
                    .map_init(Scratch::default, |scratch, &index| {
                        let edge = &self.edges[index];
                        let witnesses = test_edge(
                            &self.adj,
                            edge.u,
                            edge.v,
                            edge.value,
                            self.run.terminal,
                            scratch,
                        );
                        (witnesses, scratch.cands.len())
                    })
                    .collect()
            }),
            None => due
                .iter()
                .map(|&index| {
                    let edge = &self.edges[index];
                    let witnesses = test_edge(
                        &self.adj,
                        edge.u,
                        edge.v,
                        edge.value,
                        self.run.terminal,
                        &mut self.scratch,
                    );
                    (witnesses, self.scratch.cands.len())
                })
                .collect(),
        };
        for &(_, size) in &results {
            self.stats.max_common_neighborhood = self.stats.max_common_neighborhood.max(size);
        }
        results
    }

    fn select(&mut self, due: &[usize], results: &[(Option<Witnesses>, usize)]) -> Vec<usize> {
        let round = self.stats.epochs;
        let mut selected = Vec::new();
        for (result_index, &edge_index) in due.iter().enumerate() {
            if results[result_index].0.is_none() || self.blocked[edge_index] == round {
                continue;
            }
            selected.push(result_index);
            let edge = &self.edges[edge_index];
            let walked = for_each_induced_edge(
                &self.adj,
                edge.u,
                edge.v,
                &mut self.scratch,
                None,
                |index| self.blocked[index] = round,
            );
            debug_assert!(walked);
        }
        selected
    }

    fn commit(
        &mut self,
        due: &[usize],
        selected: &[usize],
        results: &mut [(Option<Witnesses>, usize)],
    ) {
        let mut test_all_next = false;
        for &result_index in selected {
            let edge_index = due[result_index];
            self.commit_one(
                edge_index,
                results[result_index]
                    .0
                    .take()
                    .expect("selected edge has witnesses"),
                &mut test_all_next,
            );
        }
        self.test_all = test_all_next;
    }

    fn commit_one(&mut self, index: usize, witnesses: Witnesses, test_all_next: &mut bool) {
        let edge = &self.edges[index];
        let (u, v, value) = (edge.u, edge.v, edge.value);
        self.stats.witness_segments += witnesses.len();
        self.steps.push(RemovalStep {
            u,
            v,
            value,
            position: SchedulePosition::Round(self.stats.epochs),
            witnesses,
        });
        self.edges[index].alive = false;
        if !mark_dirty(&self.adj, &mut self.dirty, u, v, &mut self.scratch) {
            *test_all_next = true;
        }
        tombstone(&mut self.adj, u, v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges_of(m: &SparseDistanceMatrix) -> Vec<(usize, usize, f64)> {
        m.edges().collect()
    }

    fn check_invariants(r: &CollapsedRips) {
        let c = &r.certificate;
        assert_eq!(c.algorithm_version(), 2);
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
        let mut prev = 1;
        for s in c.steps() {
            assert!(s.edge().0 < s.edge().1);
            assert_eq!(s.position().number(), s.position().number());
            assert!(s.position().number() >= prev);
            prev = s.position().number();
            assert!(
                s.position().number() < r.stats.epochs,
                "final round removes nothing"
            );
            assert!(!s.witnesses().is_empty());
            assert_eq!(s.witnesses()[0].0, s.value());
            for w in s.witnesses().windows(2) {
                assert!(w[0].0 < w[1].0);
            }
            assert!(s.witnesses().iter().all(|&(_, w)| w < c.vertex_count()));
        }
        for pair in c.steps().windows(2) {
            if pair[0].position().number() == pair[1].position().number() {
                let (u0, v0) = pair[0].edge();
                let (u1, v1) = pair[1].edge();
                assert!(
                    pair[1].value() < pair[0].value()
                        || (pair[1].value() == pair[0].value() && (v0, u0) < (v1, u1)),
                    "in-round steps follow the frozen priority"
                );
            }
        }
    }

    // Complete graph on 20 vertices with values in 1..=5 and heavy ties:
    // the batches are wide and the greedy tie-breaks fire often, so any
    // order dependence in the test phase would surface here.
    fn tie_heavy_20() -> DistanceMatrix {
        let mut condensed = Vec::new();
        for i in 1..20usize {
            for j in 0..i {
                condensed.push(((i * j + i + j) % 5 + 1) as f64);
            }
        }
        DistanceMatrix::from_condensed(condensed).unwrap()
    }

    #[test]
    fn thread_counts_give_identical_results() {
        let d = tie_heavy_20();
        let base = collapse_dense_rounds_parallel(&d, None, 1).unwrap();
        check_invariants(&base);
        assert!(base.stats.removed_edges > 0);
        assert!(base.stats.epochs >= 2);
        for t in [2, 4, 8] {
            let r = collapse_dense_rounds_parallel(&d, None, t).unwrap();
            assert_eq!(base.certificate, r.certificate);
            assert_eq!(edges_of(&base.matrix), edges_of(&r.matrix));
            assert_eq!(base.stats, r.stats);
        }
    }

    // Unit K4 is a conflict clique in round 1: S(e) is the whole vertex set
    // for every edge, so all six removable edges conflict pairwise and the
    // batch width is 1. Round 1 removes only (0,1). In round 2 the S sets
    // shrink, (0,2) and (1,2) no longer conflict, and both fall with apex 3.
    // Round 3 finds the spanning star at 3 and yields nothing. v1 removes
    // the same edges but records them all in pass 1: the epochs diverge.
    #[test]
    fn k4_round_one_is_a_conflict_clique() {
        let d = DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap();
        let r = collapse_dense_rounds_parallel(&d, None, 1).unwrap();
        check_invariants(&r);
        let trace: Vec<_> = r
            .certificate
            .steps()
            .iter()
            .map(|s| (s.edge(), s.position().number(), s.witnesses().to_vec()))
            .collect();
        assert_eq!(
            trace,
            vec![
                ((0, 1), 1, vec![(1.0, 2)]),
                ((0, 2), 2, vec![(1.0, 3)]),
                ((1, 2), 2, vec![(1.0, 3)]),
            ]
        );
        assert_eq!(
            edges_of(&r.matrix),
            vec![(0, 3, 1.0), (1, 3, 1.0), (2, 3, 1.0)]
        );
        assert_eq!(r.stats.epochs, 3);
        // Round 1 tests all 6 edges, round 2 the 5 dirty survivors, round 3
        // the 3 edges dirtied by the round-2 batch.
        assert_eq!(r.stats.edge_tests, 14);
        assert_eq!(r.stats.witness_segments, 3);
        assert_eq!(r.stats.max_common_neighborhood, 2);
    }

    // Two disjoint unit K4s: the components do not conflict, so each round
    // carries one component's batch next to the other's. Round 1 has width
    // 2, one removal per component.
    #[test]
    fn disjoint_k4s_give_batch_width_two() {
        let mut triplets = Vec::new();
        for base in [0usize, 4] {
            for v in 1..4 {
                for u in 0..v {
                    triplets.push((base + u, base + v, 1.0));
                }
            }
        }
        let m = SparseDistanceMatrix::from_triplets(8, &triplets).unwrap();
        let r = collapse_sparse_rounds_parallel(&m, None, 2).unwrap();
        check_invariants(&r);
        let trace: Vec<_> = r
            .certificate
            .steps()
            .iter()
            .map(|s| (s.edge(), s.position().number()))
            .collect();
        assert_eq!(
            trace,
            vec![
                ((0, 1), 1),
                ((4, 5), 1),
                ((0, 2), 2),
                ((1, 2), 2),
                ((4, 6), 2),
                ((5, 6), 2),
            ]
        );
        assert_eq!(
            edges_of(&r.matrix),
            vec![
                (0, 3, 1.0),
                (1, 3, 1.0),
                (2, 3, 1.0),
                (4, 7, 1.0),
                (5, 7, 1.0),
                (6, 7, 1.0),
            ]
        );
        assert_eq!(r.stats.epochs, 3);
    }

    #[test]
    fn chordless_four_cycle_zero_yield() {
        let m = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0), (0, 3, 1.0)],
        )
        .unwrap();
        let r = collapse_sparse_rounds_parallel(&m, None, 4).unwrap();
        check_invariants(&r);
        assert!(r.certificate.steps().is_empty());
        assert_eq!(r.matrix.num_edges(), 4);
        assert_eq!(r.stats.epochs, 1);
        assert_eq!(r.stats.edge_tests, 4);
    }

    // In unit K4, (0,2) succeeds in round 1 but the selection of (0,1)
    // blocks it. Its round-1 witness would be (1.0, 1); the recorded step
    // sits in round 2 with apex 3, so the blocked witnesses were dropped
    // and recomputed against the next snapshot. The retest happens through
    // the ordinary dirty marking (conflict symmetry puts the blocked edge
    // inside the removed edge's S), with no special case: the exact test
    // count proves no fallback widened the round-2 test set.
    #[test]
    fn blocked_successful_edge_is_retested_later() {
        let d = DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap();
        let r = collapse_dense_rounds_parallel(&d, None, 2).unwrap();
        check_invariants(&r);
        assert!(
            r.certificate
                .steps()
                .iter()
                .any(|s| s.position().number() >= 2)
        );
        let blocked = r
            .certificate
            .steps()
            .iter()
            .find(|s| s.edge() == (0, 2))
            .unwrap();
        assert_eq!(blocked.position().number(), 2);
        assert_eq!(blocked.witnesses(), &[(1.0, 3)]);
        assert_eq!(r.stats.edge_tests, 14);
    }

    #[test]
    fn empty_and_tiny_inputs() {
        let d0 = DistanceMatrix::from_points(&[]).unwrap();
        let d1 = DistanceMatrix::from_condensed(vec![]).unwrap();
        let s1 = SparseDistanceMatrix::from_triplets(1, &[]).unwrap();
        for threads in [1, 4] {
            for r in [
                collapse_dense_rounds_parallel(&d0, None, threads).unwrap(),
                collapse_dense_rounds_parallel(&d1, None, threads).unwrap(),
                collapse_sparse_rounds_parallel(&s1, None, threads).unwrap(),
            ] {
                check_invariants(&r);
                assert_eq!(r.certificate.input_edge_count(), 0);
                assert_eq!(r.certificate.terminal_level(), 0.0);
                assert!(r.certificate.steps().is_empty());
                assert_eq!(r.stats.epochs, 1);
                assert_eq!(r.stats.edge_tests, 0);
            }
        }
    }

    #[test]
    fn invalid_thresholds_are_rejected() {
        let d = DistanceMatrix::from_condensed(vec![1.0]).unwrap();
        assert!(collapse_dense_rounds_parallel(&d, Some(-1.0), 2).is_err());
        assert!(collapse_dense_rounds_parallel(&d, Some(f64::NAN), 2).is_err());
    }
}
