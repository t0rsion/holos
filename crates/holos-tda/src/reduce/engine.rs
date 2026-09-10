use rustc_hash::FxHashMap;

use crate::budget::{Region, workers};
use crate::combinadic::BinomialTable;
use crate::distances::Distances;
use crate::field::Coeffs;
use crate::simplex::Simplex;
use crate::{Diagram, Result, RipsParams};

use super::{Engine, Pivots, RawH1Class};

impl<'a, C: Coeffs + Sync, D: Distances + Sync> Engine<'a, C, D> {
    pub(crate) fn new(dist: &'a D, params: &'a RipsParams, ops: C) -> Result<Self> {
        let pool = if params.threads > 1 {
            Some(
                rayon::ThreadPoolBuilder::new()
                    .num_threads(params.threads)
                    .build()
                    .map_err(|e| crate::Error::Io(format!("thread pool: {e}")))?,
            )
        } else {
            None
        };
        Self::new_in(dist, params, ops, pool)
    }

    /// Build the engine on a caller-provided pool (or serially on `None`).
    /// The collapse pipeline shares one pool between the collapse
    /// and the reduction through this entry.
    pub(crate) fn new_in(
        dist: &'a D,
        params: &'a RipsParams,
        ops: C,
        pool: Option<rayon::ThreadPool>,
    ) -> Result<Self> {
        let n = dist.len();
        let threshold = params.threshold.unwrap_or_else(|| dist.default_threshold());
        // A complex on n points has no simplex above dimension n-1. The clamp
        // also bounds the binomial table for oversized max_dim requests.
        let max_dim = params.max_dim.min(n.saturating_sub(1));
        let bt = BinomialTable::new(n, max_dim + 2)?;
        // Packing the coefficient into the entry leaves fewer index bits.
        if bt.get(n, max_dim + 2) > ops.max_index() {
            return Err(crate::Error::IndexOverflow { n, dim: max_dim });
        }
        let effective_threshold = if threshold == f64::INFINITY {
            f64::MAX
        } else {
            threshold
        };
        let adjacency = (max_dim > 0 && params.use_apparent_pairs && params.use_adjacency_rows)
            .then(|| crate::adjacency::Adjacency::build(dist, effective_threshold))
            .flatten();
        Ok(Self {
            dist,
            bt,
            n,
            effective_threshold,
            max_dim,
            params,
            ops,
            pool,
            adjacency,
        })
    }

    /// Workers for one parallel region, under the run's thread budget.
    /// One means the serial path.
    pub(crate) fn workers(&self, region: Region, work: usize) -> usize {
        match &self.pool {
            Some(_) => workers(region, work, self.params.threads),
            None => 1,
        }
    }

    /// Run `f` on the run-wide worker pool (or inline when serial).
    pub(crate) fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        match &self.pool {
            Some(pool) => pool.install(f),
            None => f(),
        }
    }

    pub(crate) fn run(&self, diagram: &mut Diagram) {
        let edges = self.edges();
        let mut columns = self.dim0_pairs(&edges, diagram);

        // `simplices` holds all dim-d simplices in the complex, the seed for
        // canonical (d+1)-cofacet assembly.
        let mut simplices = edges;
        let mut prev_pivots: Pivots = FxHashMap::default();

        for dim in 1..=self.max_dim {
            let budget = self.workers(Region::Reduce, columns.len());
            let pivots = if budget > 1 {
                let (pivots, bars) =
                    self.reduce_dimension_parallel(&columns, dim, &prev_pivots, budget);
                diagram.bars.extend(bars);
                pivots
            } else {
                self.reduce_dimension(&columns, dim, &prev_pivots, diagram)
            };
            if dim < self.max_dim {
                (simplices, columns) =
                    self.assemble(&simplices, dim + 1, &pivots, dim + 1 < self.max_dim);
            }
            prev_pivots = pivots;
        }
    }

    /// Run the engine and retain the canonical serial reduction columns for
    /// positive H1 intervals. Other dimensions keep their usual parallel
    /// routing.
    pub(crate) fn run_with_h1_classes(&self, diagram: &mut Diagram, classes: &mut Vec<RawH1Class>) {
        let edges = self.edges();
        let mut columns = self.dim0_pairs(&edges, diagram);
        let mut simplices = edges;
        let mut prev_pivots: Pivots = FxHashMap::default();

        for dim in 1..=self.max_dim {
            let pivots = if dim == 1 {
                self.reduce_dimension_impl(&columns, dim, &prev_pivots, diagram, Some(classes))
            } else {
                let budget = self.workers(Region::Reduce, columns.len());
                if budget > 1 {
                    let (pivots, bars) =
                        self.reduce_dimension_parallel(&columns, dim, &prev_pivots, budget);
                    diagram.bars.extend(bars);
                    pivots
                } else {
                    self.reduce_dimension(&columns, dim, &prev_pivots, diagram)
                }
            };
            if dim < self.max_dim {
                (simplices, columns) =
                    self.assemble(&simplices, dim + 1, &pivots, dim + 1 < self.max_dim);
            }
            prev_pivots = pivots;
        }
    }

    #[inline]
    pub(crate) fn in_complex(&self, diameter: f64) -> bool {
        diameter <= self.effective_threshold
    }

    pub(super) fn edges(&self) -> Vec<Simplex> {
        let mut edges = Vec::new();
        let bt = &self.bt;
        let threshold = self.effective_threshold;
        self.dist.for_each_edge(|i, j, d| {
            if d <= threshold {
                edges.push(Simplex {
                    diameter: d,
                    index: bt.get(i, 2) + j as u64,
                });
            }
        });
        edges
    }
}
