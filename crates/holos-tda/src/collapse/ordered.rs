//! The ordered schedule: speculative parallel execution of the serial
//! schedule, written as algorithm version 1 certificates.
//!
//! Workers test windows of due edges in parallel against a frozen graph
//! state. The retirement walk then commits them strictly in the serial
//! order. It reuses a cached verdict only when no later committed
//! removal conflicts with the test. It recomputes an invalidated verdict
//! once against the current graph. Every worker count therefore produces
//! the serial matrix and certificate, field for field, floats bit for
//! bit. Only the work counters and the timings depend on the window and
//! worker configuration.

use std::time::{Duration, Instant};

use rayon::prelude::*;

use super::{
    AdjEntry, CollapseStats, CollapseTimings, CollapsedRips, EdgeRec, Execution, Prepared,
    RemovalStep, Run, SchedulePosition, Scratch, build_pool, finish, mark_dirty, prepare,
    test_edge, tombstone,
};
use crate::distances::Distances;
use crate::{DistanceMatrix, Result, SparseDistanceMatrix};

/// Window members per worker. Chosen from disclosed development
/// measurements (three dev clouds, 4 workers, k in {2, 8, 32, 128, 512}:
/// 32 was the best or tied-best everywhere) and frozen before the
/// registered screen. It changes work, memory, and timing only, never an
/// output field.
const WINDOW_PER_WORKER: usize = 32;

/// Upper bound on the window, so speculative storage stays small on high
/// worker counts.
const WINDOW_CAP: usize = 4096;

/// The piecewise witness segments of one removable edge.
type Witnesses = Vec<(f64, usize)>;

/// The production window for a worker count.
fn window_for(workers: usize) -> usize {
    (WINDOW_PER_WORKER * workers.max(1)).clamp(1, WINDOW_CAP)
}

/// A duration in nanoseconds, saturated at the reported field's width.
fn nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// Collapse a dense distance matrix with the ordered schedule.
///
/// The output equals [`super::collapse_dense`] at any `threads`. 0 and 1
/// run the serial implementation. A standalone call owns its thread pool.
pub fn collapse_dense_ordered_parallel(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, None)
}

/// Collapse a sparse distance matrix with the ordered schedule.
///
/// Same contract as [`collapse_dense_ordered_parallel`]. `None` keeps
/// every listed edge.
pub fn collapse_sparse_ordered_parallel(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, None)
}

/// Collapse a dense distance matrix with the ordered schedule and a forced
/// window size.
///
/// Hidden and unstable. It exists for the invariance gates, which cross
/// worker counts with window sizes. The window is not public API and no
/// output field depends on it. A `window` of 0 becomes 1.
#[doc(hidden)]
pub fn collapse_dense_ordered_with_window(
    dist: &DistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, Some(window.max(1)))
}

/// Collapse a sparse distance matrix with the ordered schedule and a forced
/// window size.
///
/// Same contract as [`collapse_dense_ordered_with_window`]: hidden,
/// unstable, and for the invariance gates only.
#[doc(hidden)]
pub fn collapse_sparse_ordered_with_window(
    dist: &SparseDistanceMatrix,
    threshold: Option<f64>,
    threads: usize,
    window: usize,
) -> Result<CollapsedRips> {
    collapse_ordered_owned(dist, threshold, threads, Some(window.max(1)))
}

/// Build an owned pool for the call and run the ordered collapse on it.
/// One worker has nothing to speculate on, so it runs the serial
/// implementation.
fn collapse_ordered_owned<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    threads: usize,
    window: Option<usize>,
) -> Result<CollapsedRips> {
    if threads.max(1) == 1 {
        return serial(dist, threshold);
    }
    let pool = build_pool(threads)?;
    let w = window.unwrap_or_else(|| window_for(pool.current_num_threads()));
    collapse_ordered_core(dist, threshold, Some(&pool), w)
}

/// The ordered collapse on a caller-provided pool (`None` runs the
/// serial implementation). The pipeline shares its run-wide pool
/// through here; the public wrappers build and own one.
pub(crate) fn collapse_ordered_in<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    pool: Option<&rayon::ThreadPool>,
) -> Result<CollapsedRips> {
    match pool.filter(|p| p.current_num_threads() > 1) {
        Some(p) => collapse_ordered_core(
            dist,
            threshold,
            Some(p),
            window_for(p.current_num_threads()),
        ),
        None => serial(dist, threshold),
    }
}

/// The serial version 1 run, reported as an ordered run. Its logical
/// trace is its own test sequence, so the logical count is the physical
/// count. Window counters stay at zero.
fn serial<D: Distances>(dist: &D, threshold: Option<f64>) -> Result<CollapsedRips> {
    super::collapse_impl(dist, threshold)
}

/// The staged-window execution of the serial schedule.
///
/// A pass runs as a sequence of stages. FORM collects up to `window` due
/// positions from the cursor without touching a dirty flag. TEST
/// evaluates the predicate for every member against the frozen graph.
/// RETIRE walks every position of the window span in schedule order and
/// commits it exactly as version 1 would, reusing a member's cached
/// verdict when no earlier removal of this stage conflicts with it.
fn collapse_ordered_core<D: Distances + Sync>(
    dist: &D,
    threshold: Option<f64>,
    pool: Option<&rayon::ThreadPool>,
    window: usize,
) -> Result<CollapsedRips> {
    OrderedExecution::new(prepare(dist, threshold)?).run(pool, window.max(1))
}

struct SpeculativeStage {
    members: Vec<usize>,
    cached: Vec<(Option<Witnesses>, usize)>,
    stale: Vec<bool>,
    last: usize,
}

struct OrderedExecution {
    edges: Vec<EdgeRec>,
    adj: Vec<Vec<AdjEntry>>,
    run: Run,
    stats: CollapseStats,
    steps: Vec<RemovalStep>,
    scratch: Scratch,
    dirty: Vec<bool>,
    test_all: bool,
    predicate_time: Duration,
    retirement_time: Duration,
    repair_time: Duration,
}

impl OrderedExecution {
    fn new(prepared: Prepared) -> Self {
        let edge_count = prepared.edges.len();
        Self {
            edges: prepared.edges,
            adj: prepared.adj,
            run: prepared.run,
            stats: CollapseStats::new(edge_count),
            steps: Vec::new(),
            scratch: Scratch::default(),
            dirty: vec![false; edge_count],
            test_all: true,
            predicate_time: Duration::ZERO,
            retirement_time: Duration::ZERO,
            repair_time: Duration::ZERO,
        }
    }

    fn run(mut self, pool: Option<&rayon::ThreadPool>, window: usize) -> Result<CollapsedRips> {
        while self.run_pass(pool, window) {}
        let timings = CollapseTimings {
            predicate_ns: nanos(self.predicate_time),
            retirement_ns: nanos(self.retirement_time),
            repair_ns: nanos(self.repair_time),
        };
        finish(
            self.run,
            Execution::Ordered,
            &self.edges,
            self.steps,
            self.stats,
            timings,
        )
    }

    fn run_pass(&mut self, pool: Option<&rayon::ThreadPool>, window: usize) -> bool {
        self.stats.epochs += 1;
        let mut removed_any = false;
        let mut test_all_next = false;
        let mut cursor = 0usize;
        while let Some(mut stage) = self.form_stage(cursor, window) {
            self.test_stage(pool, &mut stage);
            removed_any |= self.retire_stage(cursor, &mut stage, &mut test_all_next);
            // Retirement can arm a position scanned during formation. The
            // next stage therefore starts after the last member, not after
            // the formation scan.
            cursor = stage.last + 1;
        }
        self.test_all = test_all_next;
        removed_any
    }

    fn form_stage(&mut self, cursor: usize, window: usize) -> Option<SpeculativeStage> {
        let mut members = Vec::with_capacity(window);
        let mut scan = cursor;
        while scan < self.edges.len() && members.len() < window {
            if self.edges[scan].alive && (self.test_all || self.dirty[scan]) {
                members.push(scan);
            }
            scan += 1;
        }
        let last = members.last().copied()?;
        self.stats.window_batches += 1;
        self.stats.window_slots_offered = self.stats.window_slots_offered.saturating_add(window);
        self.stats.window_members_formed += members.len();
        Some(SpeculativeStage {
            stale: vec![false; members.len()],
            members,
            cached: Vec::new(),
            last,
        })
    }

    fn test_stage(&mut self, pool: Option<&rayon::ThreadPool>, stage: &mut SpeculativeStage) {
        self.stats.edge_tests += stage.members.len();
        let started = Instant::now();
        stage.cached = match pool {
            Some(pool) => pool.install(|| {
                stage
                    .members
                    .par_iter()
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
            None => stage
                .members
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
        self.predicate_time += started.elapsed();
        for &(_, size) in &stage.cached {
            self.stats.max_common_neighborhood = self.stats.max_common_neighborhood.max(size);
        }
    }

    fn retire_stage(
        &mut self,
        cursor: usize,
        stage: &mut SpeculativeStage,
        test_all_next: &mut bool,
    ) -> bool {
        let started = Instant::now();
        let mut removed_any = false;
        let mut next_member = 0usize;
        for position in cursor..=stage.last {
            let slot = stage_slot(stage, position, &mut next_member);
            removed_any |= self.retire_position(position, slot, next_member, stage, test_all_next);
        }
        self.retirement_time += started.elapsed();
        removed_any
    }

    fn retire_position(
        &mut self,
        position: usize,
        slot: Option<usize>,
        next_member: usize,
        stage: &mut SpeculativeStage,
        test_all_next: &mut bool,
    ) -> bool {
        if !self.edges[position].alive || !(self.test_all || self.dirty[position]) {
            return false;
        }
        self.stats.logical_tests += 1;
        self.dirty[position] = false;
        let edge = &self.edges[position];
        let (u, v, value) = (edge.u, edge.v, edge.value);
        let Some(witnesses) = self.retirement_witnesses(slot, stage, u, v, value) else {
            return false;
        };
        self.edges[position].alive = false;
        self.invalidate_stage(stage, next_member, u, v, test_all_next);
        self.commit_removal(u, v, value, witnesses);
        true
    }

    fn retirement_witnesses(
        &mut self,
        slot: Option<usize>,
        stage: &mut SpeculativeStage,
        u: usize,
        v: usize,
        value: f64,
    ) -> Option<Witnesses> {
        if let Some(index) = slot.filter(|&index| !stage.stale[index]) {
            self.stats.window_members_reused += 1;
            return stage.cached[index].0.take();
        }
        self.stats.edge_tests += 1;
        let started = slot.is_some().then(Instant::now);
        let witnesses = test_edge(&self.adj, u, v, value, self.run.terminal, &mut self.scratch);
        if let Some(started) = started {
            self.repair_time += started.elapsed();
        }
        self.stats.max_common_neighborhood = self
            .stats
            .max_common_neighborhood
            .max(self.scratch.cands.len());
        witnesses
    }

    fn invalidate_stage(
        &mut self,
        stage: &mut SpeculativeStage,
        next_member: usize,
        u: usize,
        v: usize,
        test_all_next: &mut bool,
    ) {
        if mark_dirty(&self.adj, &mut self.dirty, u, v, &mut self.scratch) {
            self.invalidate_conflicts(stage, next_member);
        } else {
            *test_all_next = true;
            self.invalidate_remaining(stage, next_member);
        }
    }

    fn invalidate_conflicts(&mut self, stage: &mut SpeculativeStage, next_member: usize) {
        for (index, &position) in stage.members.iter().enumerate().skip(next_member) {
            if stage.stale[index] {
                continue;
            }
            let edge = &self.edges[position];
            let conflict = self.scratch.marks.binary_search(&edge.u).is_ok()
                && self.scratch.marks.binary_search(&edge.v).is_ok();
            if conflict {
                stage.stale[index] = true;
                self.stats.invalidated_results += 1;
            }
        }
    }

    fn invalidate_remaining(&mut self, stage: &mut SpeculativeStage, next_member: usize) {
        let mut dropped = 0usize;
        for stale in stage.stale.iter_mut().skip(next_member) {
            if !*stale {
                *stale = true;
                dropped += 1;
            }
        }
        if dropped > 0 {
            self.stats.global_invalidations += 1;
            self.stats.invalidated_results += dropped;
        }
    }

    fn commit_removal(&mut self, u: usize, v: usize, value: f64, witnesses: Witnesses) {
        tombstone(&mut self.adj, u, v);
        self.stats.witness_segments += witnesses.len();
        self.steps.push(RemovalStep {
            u,
            v,
            value,
            position: SchedulePosition::Pass(self.stats.epochs),
            witnesses,
        });
    }
}

fn stage_slot(stage: &SpeculativeStage, position: usize, next_member: &mut usize) -> Option<usize> {
    if *next_member < stage.members.len() && stage.members[*next_member] == position {
        *next_member += 1;
        Some(*next_member - 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
