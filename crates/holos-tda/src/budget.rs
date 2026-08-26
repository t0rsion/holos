//! Worker budget for the parallel regions of one reduction.
//!
//! [`RipsParams::threads`](crate::RipsParams::threads) is the most workers a
//! run may use, not the number every region must use. A region with little
//! work runs faster on one thread: the split, the joins, and the cold
//! per-worker scratch cost more than the work itself. Each region turns its
//! own work estimate into a worker count, and one worker means the serial
//! path.
//!
//! The constants come from timing grids on the engineering tuning set at 1,
//! 2, 4, and 8 requested workers. A decision table in the tests pins them,
//! so a changed constant fails a test instead of moving timings unannounced.

/// A parallel region of the reduction. Each region has its own work unit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Region {
    /// Sorting a simplex slice: the edge list before the dim-0 walk, and
    /// the assembled columns of one dimension. Work is the element count.
    Sort,
    /// Testing dim-0 cycle edges for a zero-apparent cofacet. Work is the
    /// cycle edge count, or the bound the dim-0 walk has before it counts
    /// them. On the activation rows the count also picks the walk's shape:
    /// one worker walks one diameter group at a time, more than one walks
    /// a block of sorted edges at a time.
    Prefilter,
    /// Cofacet assembly of one dimension. Work is the source simplex count.
    Assemble,
    /// Column reduction of one dimension. Work is the column count.
    Reduce,
}

impl Region {
    /// Work one worker needs before it earns its share of the split.
    const fn work_per_worker(self) -> usize {
        match self {
            Region::Sort => 10_000,
            Region::Prefilter => 64,
            Region::Assemble => 128,
            Region::Reduce => 16,
        }
    }
}

/// Workers to put on `work` units of `region` under a budget of `threads`.
/// The result is 1 (the serial path) up to `threads`, and never more.
pub(crate) fn workers(region: Region, work: usize, threads: usize) -> usize {
    if threads <= 1 {
        return 1;
    }
    (work / region.work_per_worker()).clamp(1, threads)
}

/// The pinned decision table: `(region, work, threads, workers)`. Each row
/// sits at a measured crossover or at a budget limit. A change to a
/// constant in `Region::work_per_worker` breaks a row, and the test names
/// it.
#[cfg(test)]
const DECISIONS: &[(Region, usize, usize, usize)] = &[
    (Region::Sort, 0, 8, 1),
    (Region::Sort, 19_999, 8, 1),
    (Region::Sort, 20_000, 8, 2),
    (Region::Sort, 20_000, 2, 2),
    (Region::Sort, 100_000, 2, 2),
    (Region::Sort, 100_000, 4, 4),
    (Region::Sort, 100_000, 8, 8),
    (Region::Sort, 235_503, 8, 8),
    (Region::Prefilter, 98, 8, 1),
    (Region::Prefilter, 127, 8, 1),
    (Region::Prefilter, 128, 8, 2),
    (Region::Prefilter, 256, 8, 4),
    (Region::Prefilter, 512, 8, 8),
    (Region::Prefilter, 1_024, 2, 2),
    (Region::Prefilter, 1_793, 8, 8),
    (Region::Prefilter, 146_240, 8, 8),
    (Region::Assemble, 255, 8, 1),
    (Region::Assemble, 256, 8, 2),
    (Region::Assemble, 1_942, 8, 8),
    (Region::Assemble, 1_942, 4, 4),
    (Region::Reduce, 4, 8, 1),
    (Region::Reduce, 31, 8, 1),
    (Region::Reduce, 32, 8, 2),
    (Region::Reduce, 48, 2, 2),
    (Region::Reduce, 48, 8, 3),
    (Region::Reduce, 110, 8, 6),
    (Region::Reduce, 614, 8, 8),
    // A budget of one keeps every region serial, whatever the work.
    (Region::Sort, 1_000_000, 1, 1),
    (Region::Reduce, 1_000_000, 1, 1),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_table_is_pinned() {
        for &(region, work, threads, want) in DECISIONS {
            let got = workers(region, work, threads);
            assert_eq!(
                got, want,
                "{region:?}: work={work} threads={threads} gives {got} workers, pinned at {want}"
            );
        }
    }

    #[test]
    fn workers_never_exceed_the_budget() {
        for region in [
            Region::Sort,
            Region::Prefilter,
            Region::Assemble,
            Region::Reduce,
        ] {
            for threads in 1..=16 {
                for work in [0, 1, 63, 1_000, 1_000_000, usize::MAX] {
                    let got = workers(region, work, threads);
                    assert!(got >= 1 && got <= threads, "{region:?} {work} {threads}");
                }
            }
        }
    }
}
