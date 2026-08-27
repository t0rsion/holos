//! The serial reduction engine: implicit persistent cohomology following
//! ripser (Bauer 2021). The engine uses the anti-transpose convention,
//! clearing, emergent and apparent pair shortcuts, lazy-cancellation heaps,
//! and on-demand regeneration of reducer columns. Simplices exist only as
//! combinadic indices. The engine never materializes a simplex list of
//! dimension `max_dim + 1`.
//!
//! The engine itself holds no mutable state. A shared kernel takes `&self`
//! plus the buffers its caller owns, so the same methods drive this serial
//! path and the parallel one in [`crate::parallel`], where each worker owns
//! its own buffers.

use std::collections::BinaryHeap;
use std::ops::ControlFlow;

use rayon::prelude::*;
use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::budget::{workers, Region};
use crate::combinadic::BinomialTable;
#[cfg(test)]
use crate::combinadic::FacetIter;
use crate::distances::Distances;
use crate::field::{Coeffs, Entry, HeapEntry};
use crate::simplex::Simplex;
use crate::union_find::UnionFind;
use crate::{Bar, Diagram, Result, RipsParams};

/// The pivot registry for one dimension: pivot coefficient and column
/// position, keyed by pivot index. Consumed as the clearing set of the next
/// dimension.
pub(crate) type Pivots = FxHashMap<u64, (u64, usize)>;

/// Vertex and distance buffers for the apparent-pair kernels. One per
/// worker: the kernels write here and nowhere else, so two workers never
/// share a buffer.
#[derive(Default)]
pub(crate) struct PairScratch {
    /// Vertices of the facet under test.
    facet: Vec<usize>,
    /// Vertices of the cofacet under test.
    cofacet: Vec<usize>,
    /// Pairwise distances of the queried simplex.
    base: PairTable,
    /// Pairwise distances of the cofacet under test.
    cofacet_table: PairTable,
    /// Distance from the added vertex to each queried simplex vertex.
    added: Vec<f64>,
}

/// Pairwise distances of one simplex, keyed by vertex position: the distance
/// between positions p and q with p < q sits at q * (q - 1) / 2 + p.
///
/// A table fills one vertex position at a time, so a caller that stops at an
/// early facet reads only the distances that facet needed. `owner` names the
/// simplex the entries belong to, so a second query on the same simplex
/// reads nothing.
#[derive(Default)]
pub(crate) struct PairTable {
    d: Vec<f64>,
    /// Vertex positions whose distances are present. Every pair below this
    /// position is in `d`.
    filled: usize,
    /// The simplex the entries describe, as (index, vertex count).
    owner: Option<(u64, usize)>,
}

impl PairTable {
    fn holds(&self, owner: (u64, usize)) -> bool {
        self.owner == Some(owner)
    }

    fn reset(&mut self, owner: (u64, usize)) {
        self.d.clear();
        self.filled = 0;
        self.owner = Some(owner);
    }

    /// Distance between positions `p` and `q`, with `p` below `q`.
    #[inline]
    fn at(&self, p: usize, q: usize) -> f64 {
        self.d[q * (q - 1) / 2 + p]
    }

    /// Record an edge's diameter as its one pairwise distance.
    fn set_edge(&mut self, diameter: f64) {
        self.d.push(diameter);
        self.filled = 2;
    }

    /// Read the distances of the vertex positions below `upto` that are
    /// still missing. Each read takes the lower vertex first.
    fn fill<D: Distances>(&mut self, dist: &D, vertices: &[usize], upto: usize) {
        while self.filled < upto {
            let q = self.filled;
            let vq = vertices[q];
            for &vp in &vertices[..q] {
                self.d.push(dist.get(vp, vq));
            }
            self.filled += 1;
        }
    }

    /// Fill from a base simplex's table and the distances from one added
    /// vertex to each base vertex. `at` is the added vertex's position in
    /// the cofacet. Reads no distance.
    fn fill_cofacet(&mut self, owner: (u64, usize), base: &PairTable, added: &[f64], at: usize) {
        let m = added.len() + 1;
        self.reset(owner);
        for q in 0..m {
            for p in 0..q {
                let x = if q == at {
                    added[p]
                } else if p == at {
                    added[q - 1]
                } else {
                    let base_p = if p < at { p } else { p - 1 };
                    let base_q = if q < at { q } else { q - 1 };
                    base.at(base_p, base_q)
                };
                self.d.push(x);
            }
        }
        self.filled = m;
    }

    /// Largest distance between two vertex positions below `m`, skipping
    /// position `omit`. The empty maximum is 0: a simplex with fewer than
    /// two vertices has diameter 0.
    fn omit_max(&self, m: usize, omit: usize) -> f64 {
        let mut d = 0.0f64;
        for q in 0..m {
            if q == omit {
                continue;
            }
            for p in 0..q {
                if p != omit {
                    d = d.max(self.at(p, q));
                }
            }
        }
        d
    }
}

/// Which partner of a zero-apparent pair the caller asks for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pairing {
    /// The facet whose zero-apparent cofacet is the queried simplex.
    Facet,
    /// The cofacet whose zero-apparent facet is the queried simplex.
    Cofacet,
}

/// A confirmed zero-apparent pair, seen from the queried simplex. The
/// classifier returns it only after both halves of the test agree, so a
/// caller that holds one never repeats the test. The partner's vertex set
/// stays in the scratch the call was given.
#[derive(Clone, Copy)]
pub(crate) struct ApparentPair {
    /// The partner: the facet under [`Pairing::Facet`], the cofacet under
    /// [`Pairing::Cofacet`].
    pub(crate) other: Simplex,
    /// Position of the differing vertex in the larger of the two simplices.
    /// Under [`Pairing::Facet`] it is the boundary sign exponent.
    pub(crate) k: usize,
}

pub(crate) struct Engine<'a, C: Coeffs, D: Distances> {
    pub(crate) dist: &'a D,
    pub(crate) bt: BinomialTable,
    pub(crate) n: usize,
    /// The filtration threshold, with `+inf` replaced by `f64::MAX`. A
    /// diameter is in the complex exactly when it is at or below this value:
    /// `+inf` fails against `f64::MAX`, and no finite diameter exceeds it.
    /// That folds the finiteness test into the threshold test.
    effective_threshold: f64,
    pub(crate) max_dim: usize,
    pub(crate) params: &'a RipsParams,
    pub(crate) ops: C,
    /// One worker pool for the whole run. Every parallel region installs onto
    /// it, so the pool is built once and the thread count is honored exactly.
    /// `None` runs serially.
    pool: Option<rayon::ThreadPool>,
    /// Adjacency bitsets for the dim-0 apparent test, when the rows fit
    /// the budget.
    adjacency: Option<crate::adjacency::Adjacency>,
}

/// Sorted edges per block of the parallel dim-0 walk on activation rows.
const DIM0_ROWS_BLOCK: usize = 8192;
/// Sorted edges per block of the serial walk on activation rows.
const DIM0_ROWS_BLOCK_SERIAL: usize = 512;

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
        // Check that every simplex index the run can produce still fits.
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

    #[inline]
    pub(crate) fn in_complex(&self, diameter: f64) -> bool {
        diameter <= self.effective_threshold
    }

    fn edges(&self) -> Vec<Simplex> {
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

    fn sorted_edge_keys(&self, edges: &[Simplex]) -> Vec<u128> {
        let mut sorted: Vec<u128> = edges.iter().map(|&edge| edge_key(edge)).collect();
        match &self.pool {
            Some(pool) if self.workers(Region::Sort, sorted.len()) > 1 => {
                pool.install(|| sorted.par_sort_unstable())
            }
            _ => sorted.sort_unstable(),
        }
        sorted
    }

    fn walk_dim0_edge(
        &self,
        edge: Simplex,
        ends: [usize; 2],
        union_find: &mut UnionFind,
        diagram: &mut Diagram,
    ) -> bool {
        let roots = (union_find.find(ends[0]), union_find.find(ends[1]));
        if roots.0 == roots.1 {
            return self.max_dim > 0;
        }
        if edge.diameter > 0.0 {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death: edge.diameter,
            });
        }
        union_find.link(roots.0, roots.1);
        false
    }

    fn finish_dim0(&self, union_find: &mut UnionFind, diagram: &mut Diagram) {
        for vertex in 0..self.n {
            if union_find.find(vertex) == vertex {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death: f64::INFINITY,
                });
            }
        }
    }

    /// Emit the dim-0 bars with one union-find pass and return the dim-1
    /// columns (cycle edges), sorted for reduction (diameter descending,
    /// index ascending).
    ///
    /// The walk stays serial: its result depends on the order. On one thread
    /// the apparent test runs inside the walk. With workers it runs after
    /// the walk, on the pool.
    fn dim0_pairs(&self, edges: &[Simplex], diagram: &mut Diagram) -> Vec<Simplex> {
        let sorted = self.sorted_edge_keys(edges);
        // A deferred test decodes its edge a second time. Only workers pay
        // that back, so the test runs inside the walk, on the vertices the
        // walk already holds, whenever the deferred test would run serially.
        // The walk has not counted the cycle edges yet. A spanning forest
        // holds at most n-1 edges, so it finds at least `edges - (n - 1)`
        // of them, and that bound sizes the deferred test.
        let cycle_bound = edges.len().saturating_sub(self.n.saturating_sub(1));
        if let Some(adjacency) = &self.adjacency {
            return self.dim0_pairs_by_rows(&sorted, adjacency, diagram);
        }
        let defer = self.workers(Region::Prefilter, cycle_bound) > 1;
        let mut uf = UnionFind::new(self.n);
        let mut cycles: Vec<(Simplex, [usize; 2])> = Vec::new();
        let mut columns = Vec::new();
        let mut verts = Vec::new();
        let mut pairs = PairScratch::default();
        for &key in &sorted {
            let edge = edge_from_key(key);
            self.bt.unrank(edge.index, 1, self.n, &mut verts);
            let ends = [verts[0], verts[1]];
            if self.walk_dim0_edge(edge, ends, &mut uf, diagram) {
                if defer {
                    cycles.push((edge, ends));
                } else if self.is_dim0_column(&verts, edge, &mut pairs) {
                    columns.push(edge);
                }
            }
        }
        self.finish_dim0(&mut uf, diagram);
        if defer {
            columns = self.dim0_columns(cycles);
        }
        columns.reverse();
        columns
    }

    fn row_block_end(sorted: &[u128], start: usize, block: usize) -> usize {
        let mut end = (start + block).min(sorted.len());
        while end < sorted.len() && sorted[end] >> 64 == sorted[end - 1] >> 64 {
            end += 1;
        }
        end
    }

    fn decode_edge_ends(&self, keys: &[u128], workers: usize, ends: &mut Vec<[usize; 2]>) {
        ends.clear();
        if workers <= 1 {
            let mut vertices = Vec::new();
            for &key in keys {
                self.bt
                    .unrank(edge_from_key(key).index, 1, self.n, &mut vertices);
                ends.push([vertices[0], vertices[1]]);
            }
            return;
        }
        ends.resize(keys.len(), [0, 0]);
        let chunk = (keys.len() / (workers * 4)).clamp(64, 4096);
        self.install(|| {
            ends.par_chunks_mut(chunk)
                .zip(keys.par_chunks(chunk))
                .for_each_init(Vec::new, |vertices, (slots, part)| {
                    for (slot, &key) in slots.iter_mut().zip(part) {
                        self.bt
                            .unrank(edge_from_key(key).index, 1, self.n, vertices);
                        *slot = [vertices[0], vertices[1]];
                    }
                });
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn walk_row_block(
        &self,
        keys: &[u128],
        ends: &[[usize; 2]],
        rows: &crate::adjacency::Rows,
        adjacency: &crate::adjacency::Adjacency,
        workers: usize,
        union_find: &mut UnionFind,
        diagram: &mut Diagram,
        columns: &mut Vec<Simplex>,
        cycles: &mut Vec<(Simplex, [usize; 2])>,
    ) {
        cycles.clear();
        for (&key, &edge_ends) in keys.iter().zip(ends) {
            let edge = edge_from_key(key);
            if !self.walk_dim0_edge(edge, edge_ends, union_find, diagram) {
                continue;
            }
            if workers > 1 {
                cycles.push((edge, edge_ends));
            } else if !rows.pairs_edge(adjacency, edge_ends[0], edge_ends[1], edge.diameter) {
                columns.push(edge);
            }
        }
    }

    fn filter_row_cycles(
        &self,
        rows: &crate::adjacency::Rows,
        adjacency: &crate::adjacency::Adjacency,
        workers: usize,
        cycles: &[(Simplex, [usize; 2])],
        columns: &mut Vec<Simplex>,
        keep: &mut Vec<u8>,
    ) {
        if cycles.is_empty() {
            return;
        }
        keep.clear();
        keep.resize(cycles.len(), 0);
        let chunk = (cycles.len() / (workers * 4)).clamp(64, 4096);
        self.install(|| {
            keep.par_chunks_mut(chunk)
                .zip(cycles.par_chunks(chunk))
                .for_each(|(slots, part)| {
                    for (slot, (edge, ends)) in slots.iter_mut().zip(part) {
                        *slot =
                            u8::from(!rows.pairs_edge(adjacency, ends[0], ends[1], edge.diameter));
                    }
                });
        });
        columns.extend(
            cycles
                .iter()
                .zip(keep.iter())
                .filter_map(|((edge, _), &value)| (value != 0).then_some(*edge)),
        );
    }

    /// The dim-0 walk with the apparent test on activation rows. The walk
    /// takes the sorted edges one diameter at a time: it first sets the bit
    /// of every edge of that diameter in both ends' rows, so the rows hold
    /// exactly the edges at or below the diameter under test, then it walks
    /// the group. For a cycle edge `(u, v)` the largest common bit of the two
    /// rows is the largest `w` with both other edges at or below the
    /// diameter, which is the youngest cofacet of equal diameter; the facet
    /// check reads at most two distances from `adjacency`. The columns and
    /// their order are those of the plain walk.
    fn dim0_pairs_by_rows(
        &self,
        sorted: &[u128],
        adjacency: &crate::adjacency::Adjacency,
        diagram: &mut Diagram,
    ) -> Vec<Simplex> {
        let cycle_bound = sorted.len().saturating_sub(self.n.saturating_sub(1));
        let workers = self.workers(Region::Prefilter, cycle_bound);
        // A block of edges at a time, closed at a diameter boundary. The
        // rows then also hold the block's later edges, and the test verifies
        // each candidate against the diameter under test, so a block never
        // changes an answer. Serial blocks are small: the walk pays a loop
        // per block, and a large block gives the serial test more candidates
        // above the diameter to reject.
        let block = if workers > 1 {
            DIM0_ROWS_BLOCK
        } else {
            DIM0_ROWS_BLOCK_SERIAL
        };
        let mut rows = crate::adjacency::Rows::new(self.n);
        let mut uf = UnionFind::new(self.n);
        let mut columns = Vec::new();
        let mut ends: Vec<[usize; 2]> = Vec::new();
        let mut cycles: Vec<(Simplex, [usize; 2])> = Vec::new();
        let mut keep: Vec<u8> = Vec::new();
        let mut start = 0;
        while start < sorted.len() {
            let end = Self::row_block_end(sorted, start, block);
            self.decode_edge_ends(&sorted[start..end], workers, &mut ends);
            for uv in &ends {
                rows.set(uv[0], uv[1]);
            }
            self.walk_row_block(
                &sorted[start..end],
                &ends,
                &rows,
                adjacency,
                workers,
                &mut uf,
                diagram,
                &mut columns,
                &mut cycles,
            );
            self.filter_row_cycles(&rows, adjacency, workers, &cycles, &mut columns, &mut keep);
            start = end;
        }
        self.finish_dim0(&mut uf, diagram);
        columns.reverse();
        columns
    }

    /// Drop the cycle edges that a zero-apparent cofacet already pairs. The
    /// rest are the dim-1 columns, in the order the union-find walk found
    /// them.
    ///
    /// Each edge is tested on its own against the frozen distances, so the
    /// tests may run in any order. Workers take chunks of one shared result
    /// slice. A worker writes only the slots of its own chunk, and the
    /// compaction then reads the slice in walk order, so the columns are the
    /// same at every worker count.
    fn dim0_columns(&self, cycles: Vec<(Simplex, [usize; 2])>) -> Vec<Simplex> {
        if !self.params.use_apparent_pairs {
            return cycles.into_iter().map(|(e, _)| e).collect();
        }
        let test =
            |(e, uv): (Simplex, [usize; 2]), verts: &mut Vec<usize>, pairs: &mut PairScratch| {
                verts.clear();
                verts.extend(uv);
                self.is_dim0_column(verts, e, pairs)
            };
        let budget = self.workers(Region::Prefilter, cycles.len());
        if budget <= 1 {
            let mut verts = Vec::new();
            let mut pairs = PairScratch::default();
            return cycles
                .into_iter()
                .filter(|&c| test(c, &mut verts, &mut pairs))
                .map(|(e, _)| e)
                .collect();
        }
        // One byte a slot, not `bool`: a worker writes its own slots, and a
        // plain byte keeps the write a store with no read behind it.
        let mut keep = vec![0u8; cycles.len()];
        let chunk = (cycles.len() / (budget * 4)).clamp(64, 4096);
        self.install(|| {
            keep.par_chunks_mut(chunk)
                .zip(cycles.par_chunks(chunk))
                .for_each_init(
                    || (Vec::new(), PairScratch::default()),
                    |(verts, pairs), (slots, part)| {
                        for (slot, &e) in slots.iter_mut().zip(part) {
                            *slot = u8::from(test(e, verts, pairs));
                        }
                    },
                );
        });
        cycles
            .into_iter()
            .zip(keep)
            .filter_map(|((e, _), keep)| (keep != 0).then_some(e))
            .collect()
    }

    /// True when edge `e` starts a column of dimension 1: no zero-apparent
    /// cofacet pairs it already. `verts` holds the vertices of `e`.
    fn is_dim0_column(&self, verts: &[usize], e: Simplex, pairs: &mut PairScratch) -> bool {
        !self.params.use_apparent_pairs
            || self
                .zero_apparent(verts, e, 1, Pairing::Cofacet, pairs)
                .is_none()
    }

    /// Canonical cofacet assembly with clearing and apparent-pair pruning.
    /// Returns (all (d)-simplices, columns to reduce in dimension d). The
    /// simplex list seeds the next dimension's assembly. The final dimension
    /// has no such consumer, so `seed_next` is false there.
    ///
    /// Generation is per-simplex independent, so it runs in parallel over
    /// chunks. Concatenating the seed lists in chunk order matches the serial
    /// seed list. The columns are then sorted into reduction order.
    fn assemble(
        &self,
        simplices: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        seed_next: bool,
    ) -> (Vec<Simplex>, Vec<Simplex>) {
        let column_order = |a: &Simplex, b: &Simplex| {
            b.diameter
                .total_cmp(&a.diameter)
                .then(a.index.cmp(&b.index))
        };

        let budget = self.workers(Region::Assemble, simplices.len());
        if budget <= 1 {
            let (next_simplices, mut columns) =
                self.assemble_chunk(simplices, dim, prev_pivots, seed_next);
            columns.sort_unstable_by(column_order);
            return (next_simplices, columns);
        }

        let chunk = (simplices.len() / (budget * 4)).clamp(64, 4096);
        self.install(|| {
            let parts: Vec<(Vec<Simplex>, Vec<Simplex>)> = simplices
                .par_chunks(chunk)
                .map(|c| self.assemble_chunk(c, dim, prev_pivots, seed_next))
                .collect();
            let mut next_simplices = Vec::new();
            let mut columns = Vec::new();
            for (part_next, part_cols) in parts {
                next_simplices.extend(part_next);
                columns.extend(part_cols);
            }
            if self.workers(Region::Sort, columns.len()) > 1 {
                columns.par_sort_unstable_by(column_order);
            } else {
                columns.sort_unstable_by(column_order);
            }
            (next_simplices, columns)
        })
    }

    /// Generate the in-complex cofacets of one slice of simplices: the next
    /// dimension's seed simplices and its reducible columns (unsorted).
    fn assemble_chunk(
        &self,
        simplices: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        seed_next: bool,
    ) -> (Vec<Simplex>, Vec<Simplex>) {
        let mut next_simplices = Vec::new();
        let mut columns = Vec::new();
        let mut verts = Vec::new();
        let mut cofacet_verts = Vec::new();
        let mut pairs = PairScratch::default();
        for s in simplices {
            self.bt.unrank(s.index, dim - 1, self.n, &mut verts);
            self.dist.for_each_cofacet_bounded(
                &self.bt,
                *s,
                &verts,
                dim - 1,
                true,
                self.effective_threshold,
                |cf| {
                    let cofacet = Simplex {
                        diameter: cf.diameter,
                        index: cf.index,
                    };
                    if seed_next {
                        next_simplices.push(cofacet);
                    }
                    let cleared = self.params.use_clearing && prev_pivots.contains_key(&cf.index);
                    if !cleared {
                        // `upper_only` puts the added vertex above every
                        // simplex vertex, so the cofacet's vertex set is the
                        // simplex's with that vertex appended.
                        let apparent = self.params.use_apparent_pairs && {
                            insert_vertex(&mut cofacet_verts, &verts, cf.vertex, verts.len());
                            self.is_in_zero_apparent_pair(&cofacet_verts, cofacet, dim, &mut pairs)
                        };
                        if !apparent {
                            columns.push(cofacet);
                        }
                    }
                    ControlFlow::<()>::Continue(())
                },
            );
        }
        (next_simplices, columns)
    }

    #[allow(clippy::too_many_arguments)]
    fn record_emergent_pair(
        &self,
        column: Simplex,
        column_position: usize,
        dim: usize,
        pivot: Option<Entry>,
        working_coboundary: &BinaryHeap<HeapEntry>,
        working_reduction: &BinaryHeap<HeapEntry>,
        pivots: &mut Pivots,
        diagram: &mut Diagram,
    ) -> bool {
        let Some(pivot) = pivot else {
            return false;
        };
        if !working_coboundary.is_empty() {
            return false;
        }
        debug_assert!(working_reduction.is_empty());
        self.emit_pair(column, self.ops.simplex(pivot), dim, diagram);
        pivots.insert(
            self.ops.index(pivot),
            (self.ops.coeff(pivot), column_position),
        );
        true
    }

    fn record_essential_column(
        &self,
        column: Simplex,
        dim: usize,
        previous_pivots: &Pivots,
        diagram: &mut Diagram,
    ) {
        let prior_death = !self.params.use_clearing && previous_pivots.contains_key(&column.index);
        if !prior_death {
            diagram.bars.push(Bar {
                dim,
                birth: column.diameter,
                death: f64::INFINITY,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_initialized_column(
        &self,
        column: Simplex,
        column_position: usize,
        dim: usize,
        previous_pivots: &Pivots,
        columns: &[Simplex],
        pivots: &mut Pivots,
        reduction_entries: &mut Vec<Entry>,
        reduction_offsets: &[usize],
        working_coboundary: &mut BinaryHeap<HeapEntry>,
        working_reduction: &mut BinaryHeap<HeapEntry>,
        vertices: &mut Vec<usize>,
        pairs: &mut PairScratch,
        mut pivot: Option<Entry>,
        diagram: &mut Diagram,
    ) {
        loop {
            let Some(entry) = pivot else {
                self.record_essential_column(column, dim, previous_pivots, diagram);
                return;
            };
            let index = self.ops.index(entry);
            if let Some(&(other_coeff, other_position)) = pivots.get(&index) {
                let range =
                    reduction_offsets[other_position]..reduction_offsets[other_position + 1];
                self.fold_reducer(
                    entry,
                    other_coeff,
                    columns[other_position],
                    &reduction_entries[range],
                    dim,
                    working_reduction,
                    working_coboundary,
                    vertices,
                );
                pivot = self.get_pivot(working_coboundary);
                continue;
            }
            if let Some(apparent) = self.reduce_apparent_facet(
                entry,
                dim,
                working_reduction,
                working_coboundary,
                vertices,
                pairs,
            ) {
                pivot = apparent;
                continue;
            }
            self.emit_pair(column, self.ops.simplex(entry), dim, diagram);
            pivots.insert(index, (self.ops.coeff(entry), column_position));
            self.drain_into(working_reduction, reduction_entries);
            return;
        }
    }

    /// Reduce the dim-d columns serially against implicit (d+1)-rows.
    pub(crate) fn reduce_dimension(
        &self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        diagram: &mut Diagram,
    ) -> Pivots {
        let mut pivot_map: Pivots =
            FxHashMap::with_capacity_and_hasher(columns.len(), FxBuildHasher);
        let mut v_entries: Vec<Entry> = Vec::new();
        let mut v_offsets: Vec<usize> = vec![0];
        let mut working_cob: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut working_red: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut cofacet_buf: Vec<Entry> = Vec::new();
        let mut verts: Vec<usize> = Vec::new();
        let mut cofacet_verts: Vec<usize> = Vec::new();
        let mut pairs = PairScratch::default();

        for (col_pos, &column) in columns.iter().enumerate() {
            working_cob.clear();
            working_red.clear();
            let pivot = self.init_coboundary(
                column,
                dim,
                |index| pivot_map.contains_key(&index),
                &mut working_cob,
                &mut cofacet_buf,
                &mut verts,
                &mut cofacet_verts,
                &mut pairs,
            );
            if !self.record_emergent_pair(
                column,
                col_pos,
                dim,
                pivot,
                &working_cob,
                &working_red,
                &mut pivot_map,
                diagram,
            ) {
                self.reduce_initialized_column(
                    column,
                    col_pos,
                    dim,
                    prev_pivots,
                    columns,
                    &mut pivot_map,
                    &mut v_entries,
                    &v_offsets,
                    &mut working_cob,
                    &mut working_red,
                    &mut verts,
                    &mut pairs,
                    pivot,
                    diagram,
                );
            }
            v_offsets.push(v_entries.len());
        }
        pivot_map
    }

    /// Record the finite bar of a birth/death pair. A zero-persistence pair
    /// emits no bar.
    pub(crate) fn emit_pair(
        &self,
        column: Simplex,
        pivot: Simplex,
        dim: usize,
        diagram: &mut Diagram,
    ) {
        if pivot.diameter > column.diameter {
            diagram.bars.push(Bar {
                dim,
                birth: column.diameter,
                death: pivot.diameter,
            });
        }
    }

    /// Fold a reducer column (its leading simplex plus V-column, each scaled to
    /// cancel pivot `p`) into the working buffers. Shared by the serial and
    /// parallel reducers so their arithmetic cannot drift apart.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn fold_reducer(
        &self,
        p: Entry,
        other_coeff: u64,
        leading: Simplex,
        v: &[Entry],
        dim: usize,
        working_red: &mut BinaryHeap<HeapEntry>,
        working_cob: &mut BinaryHeap<HeapEntry>,
        verts: &mut Vec<usize>,
    ) {
        let factor = self.ops.factor(self.ops.coeff(p), other_coeff);
        let reducer = self.ops.pack(leading.diameter, leading.index, factor);
        self.add_simplex_coboundary(reducer, dim, working_red, working_cob, verts);
        for &s in v {
            let scaled = self.ops.pack(
                s.diameter,
                self.ops.index(s),
                self.ops.mul(self.ops.coeff(s), factor),
            );
            self.add_simplex_coboundary(scaled, dim, working_red, working_cob, verts);
        }
    }

    /// Apply the apparent-pair shortcut to pivot `p` when it fits. On a hit,
    /// fold the paired facet's coboundary into the working column and return
    /// the next pivot. `None` means `p` is a genuine pivot.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn reduce_apparent_facet(
        &self,
        p: Entry,
        dim: usize,
        working_red: &mut BinaryHeap<HeapEntry>,
        working_cob: &mut BinaryHeap<HeapEntry>,
        verts: &mut Vec<usize>,
        pairs: &mut PairScratch,
    ) -> Option<Option<Entry>> {
        if !self.params.use_apparent_pairs {
            return None;
        }
        let simplex = self.ops.simplex(p);
        self.bt.unrank(simplex.index, dim + 1, self.n, verts);
        let pair = self.zero_apparent(verts, simplex, dim + 1, Pairing::Facet, pairs)?;
        // Ripser negates the facet's boundary coefficient so the pivot cancels
        // exactly.
        let coeff = self
            .ops
            .neg(self.ops.mul(self.ops.sign(pair.k), self.ops.coeff(p)));
        let e = self.ops.pack(pair.other.diameter, pair.other.index, coeff);
        // The classifier left the facet's vertices in the scratch.
        self.add_coboundary_with(e, &pairs.facet, dim, working_red, working_cob);
        Some(self.get_pivot(working_cob))
    }

    /// Enumerate the coboundary of `column` and return its pivot, as
    /// ripser's init_coboundary_and_get_pivot does. When the emergent
    /// shortcut fires, the pivot comes back without building the working
    /// column, so `working_cob` stays as the caller left it and a pivot
    /// beside an empty `working_cob` marks that case. `has_pivot` answers
    /// whether a given cofacet index is already a claimed pivot; a caller
    /// whose answer can go stale must test the pivot again itself.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn init_coboundary(
        &self,
        column: Simplex,
        dim: usize,
        has_pivot: impl Fn(u64) -> bool,
        working_cob: &mut BinaryHeap<HeapEntry>,
        cofacet_buf: &mut Vec<Entry>,
        verts: &mut Vec<usize>,
        cofacet_verts: &mut Vec<usize>,
        pairs: &mut PairScratch,
    ) -> Option<Entry> {
        self.bt.unrank(column.index, dim, self.n, verts);
        cofacet_buf.clear();
        let mut check_emergent = self.params.use_emergent_pairs;
        let emergent = self
            .dist
            .for_each_cofacet(&self.bt, column, verts, dim, false, |cf| {
                if !self.in_complex(cf.diameter) {
                    return ControlFlow::Continue(());
                }
                let cofacet = self.ops.pack(cf.diameter, cf.index, self.ops.sign(cf.k));
                cofacet_buf.push(cofacet);
                if check_emergent && cf.diameter == column.diameter {
                    // The map lookup is far cheaper than the apparent-facet
                    // test, so gate that test behind the lookup.
                    if !has_pivot(cf.index) {
                        let stolen = self.params.use_apparent_pairs && {
                            insert_vertex(cofacet_verts, verts, cf.vertex, cf.k);
                            self.zero_apparent(
                                cofacet_verts,
                                self.ops.simplex(cofacet),
                                dim + 1,
                                Pairing::Facet,
                                pairs,
                            )
                            .is_some()
                        };
                        if !stolen {
                            return ControlFlow::Break(cofacet);
                        }
                    }
                    check_emergent = false;
                }
                ControlFlow::Continue(())
            });
        if let Some(p) = emergent {
            return Some(p);
        }
        // One heapify orders the whole coboundary. The cofacets go into the
        // vector the heap already owns, so a column allocates nothing the
        // previous column did not.
        let mut heap = std::mem::take(working_cob).into_vec();
        heap.extend(cofacet_buf.iter().map(|&c| HeapEntry::new(c)));
        *working_cob = BinaryHeap::from(heap);
        self.get_pivot(working_cob)
    }

    /// Enumerate every in-complex cofacet of `column` into the working column,
    /// with no emergent shortcut. Used by the parallel path to rebuild a
    /// column whose emergent claim lost its race.
    pub(crate) fn build_full_coboundary(
        &self,
        column: Simplex,
        dim: usize,
        working_cob: &mut BinaryHeap<HeapEntry>,
        verts: &mut Vec<usize>,
    ) {
        self.bt.unrank(column.index, dim, self.n, verts);
        // The cofacets go into the vector the heap owns and one heapify
        // orders them, as in [`Engine::init_coboundary`].
        let mut heap = std::mem::take(working_cob).into_vec();
        self.dist.for_each_cofacet_bounded(
            &self.bt,
            column,
            verts,
            dim,
            false,
            self.effective_threshold,
            |cf| {
                heap.push(HeapEntry::new(self.ops.pack(
                    cf.diameter,
                    cf.index,
                    self.ops.sign(cf.k),
                )));
                ControlFlow::<()>::Continue(())
            },
        );
        *working_cob = BinaryHeap::from(heap);
    }

    /// Push a dim-d entry into the V-column. Push its regenerated coboundary,
    /// scaled by the entry's coefficient, into the working column.
    pub(crate) fn add_simplex_coboundary(
        &self,
        entry: Entry,
        dim: usize,
        working_red: &mut BinaryHeap<HeapEntry>,
        working_cob: &mut BinaryHeap<HeapEntry>,
        verts: &mut Vec<usize>,
    ) {
        self.bt.unrank(self.ops.index(entry), dim, self.n, verts);
        self.add_coboundary_with(entry, verts, dim, working_red, working_cob);
    }

    /// [`Engine::add_simplex_coboundary`] for a caller that already holds the
    /// entry's vertex set.
    pub(crate) fn add_coboundary_with(
        &self,
        entry: Entry,
        verts: &[usize],
        dim: usize,
        working_red: &mut BinaryHeap<HeapEntry>,
        working_cob: &mut BinaryHeap<HeapEntry>,
    ) {
        working_red.push(HeapEntry::new(entry));
        let ops = &self.ops;
        let entry_coeff = ops.coeff(entry);
        let threshold = self.effective_threshold;
        self.dist.for_each_cofacet_bounded(
            &self.bt,
            ops.simplex(entry),
            verts,
            dim,
            false,
            threshold,
            |cf| {
                let coeff = ops.mul_sign(cf.k, entry_coeff);
                working_cob.push(HeapEntry::new(ops.pack(cf.diameter, cf.index, coeff)));
                ControlFlow::<()>::Continue(())
            },
        );
    }

    pub(crate) fn get_pivot(&self, heap: &mut BinaryHeap<HeapEntry>) -> Option<Entry> {
        let pivot = self.ops.pop_pivot(heap)?;
        heap.push(HeapEntry::new(pivot));
        Some(pivot)
    }

    /// Drain the working reduction column, cancelled in the field, into the
    /// V store.
    pub(crate) fn drain_into(&self, heap: &mut BinaryHeap<HeapEntry>, out: &mut Vec<Entry>) {
        while let Some(e) = self.ops.pop_pivot(heap) {
            out.push(e);
        }
    }

    /// Give `table` the pairwise distances of `simplex` up to vertex
    /// position `upto`. A table that already holds them reads nothing, and
    /// an edge's one distance is its diameter, so an edge reads nothing
    /// either.
    fn base_table(&self, vertices: &[usize], simplex: Simplex, table: &mut PairTable, upto: usize) {
        let owner = (simplex.index, vertices.len());
        if !table.holds(owner) {
            table.reset(owner);
            if vertices.len() == 2 {
                debug_assert_eq!(self.dist.get(vertices[0], vertices[1]), simplex.diameter);
                table.set_edge(simplex.diameter);
            }
        }
        table.fill(self.dist, vertices, upto);
    }

    /// Read the distance from `added` to each simplex vertex, in vertex
    /// order. `at` is the position `added` takes among them, so each read
    /// takes the lower vertex first, as the diameter fold does.
    fn added_distances(&self, vertices: &[usize], added: usize, at: usize, out: &mut Vec<f64>) {
        out.clear();
        for &v in &vertices[..at] {
            out.push(self.dist.get(v, added));
        }
        for &v in &vertices[at..] {
            out.push(self.dist.get(added, v));
        }
    }

    /// Return the first facet in ripser's facet order with the same
    /// diameter, its enumerator position k for the boundary sign, and the
    /// vertex it drops. `vertices` must be the vertex set of `simplex`, and
    /// `table` must be its pairwise distances or empty. `facet_verts` ends
    /// holding the returned facet's vertices.
    ///
    /// The walk reads no distance twice and runs no binary search. Dropping
    /// vertex position k takes the facet index from `idx_below - C(v_k,
    /// k+1) + idx_above`, the identity `FacetIter` runs after its search,
    /// and the facet diameter is the largest distance in `table` that
    /// avoids position k.
    fn zero_pivot_facet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        table: &mut PairTable,
        facet_verts: &mut Vec<usize>,
    ) -> Option<(Simplex, usize, usize)> {
        let m = vertices.len();
        let mut idx_below = simplex.index;
        let mut idx_above = 0u64;
        for k in (0..m).rev() {
            // Dropping the top vertex leaves the pairs below it, so that
            // facet needs one position less than the rest.
            let upto = if k + 1 == m { m - 1 } else { m };
            self.base_table(vertices, simplex, table, upto);
            let removed = vertices[k];
            let below = self.bt.get(removed, k + 1);
            let index = idx_below - below + idx_above;
            let d = table.omit_max(m, k);
            if d == simplex.diameter {
                facet_verts.clear();
                facet_verts.extend_from_slice(&vertices[..k]);
                facet_verts.extend_from_slice(&vertices[k + 1..]);
                return Some((Simplex { diameter: d, index }, k, removed));
            }
            idx_below -= below;
            idx_above += self.bt.get(removed, k);
        }
        None
    }

    /// Return the first cofacet in descending index order with the same
    /// diameter, its enumerator position k, and the vertex it adds.
    ///
    /// No cofacet has a diameter below its simplex's, so a cofacet at or
    /// below that bound carries exactly that diameter. The bounded
    /// enumerator reports those and no others, and it drops a wider cofacet
    /// at the first distance that proves it wider.
    fn zero_pivot_cofacet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<(Simplex, usize, usize)> {
        self.dist.for_each_cofacet_bounded(
            &self.bt,
            simplex,
            vertices,
            dim,
            false,
            simplex.diameter,
            |cf| {
                ControlFlow::Break((
                    Simplex {
                        diameter: cf.diameter,
                        index: cf.index,
                    },
                    cf.k,
                    cf.vertex,
                ))
            },
        )
    }

    /// Classify `simplex` against the zero-apparent pairing, in the direction
    /// the caller asks for. `Some` means both halves agree: the partner's
    /// zero pivot in the opposite direction is `simplex` itself.
    ///
    /// `vertices` must be the vertex set of `simplex`, ascending. The kernel
    /// builds the partner's vertex set from it, by dropping one vertex or by
    /// inserting one, and never unranks the partner. On `Some` the partner's
    /// vertices stay in the scratch, in `scratch.facet` under
    /// [`Pairing::Facet`] and in `scratch.cofacet` under [`Pairing::Cofacet`].
    pub(crate) fn zero_apparent(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
        pairing: Pairing,
        scratch: &mut PairScratch,
    ) -> Option<ApparentPair> {
        let PairScratch {
            facet,
            cofacet,
            base,
            cofacet_table,
            added,
        } = scratch;
        match pairing {
            Pairing::Facet => {
                let (f, k, _) = self.zero_pivot_facet_with(vertices, simplex, base, facet)?;
                let (back, _, _) = self.zero_pivot_cofacet_with(facet, f, dim - 1)?;
                (back.index == simplex.index).then_some(ApparentPair { other: f, k })
            }
            Pairing::Cofacet => {
                let (c, k, vertex) = self.zero_pivot_cofacet_with(vertices, simplex, dim)?;
                insert_vertex(cofacet, vertices, vertex, k);
                // The cofacet's facets come off the top, and dropping the
                // added vertex leaves `simplex` itself, whose diameter is
                // the cofacet's. No facet exceeds that, so the back-check
                // stops at `simplex` at the latest. With the added vertex on
                // top it stops there at once, and reads nothing.
                if k == vertices.len() {
                    return Some(ApparentPair { other: c, k });
                }
                self.base_table(vertices, simplex, base, vertices.len());
                self.added_distances(vertices, vertex, k, added);
                cofacet_table.fill_cofacet((c.index, cofacet.len()), base, added, k);
                let (back, _, _) = self.zero_pivot_facet_with(cofacet, c, cofacet_table, facet)?;
                (back.index == simplex.index).then_some(ApparentPair { other: c, k })
            }
        }
    }

    /// True when `simplex` is either half of a zero-apparent pair.
    fn is_in_zero_apparent_pair(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
        scratch: &mut PairScratch,
    ) -> bool {
        self.zero_apparent(vertices, simplex, dim, Pairing::Cofacet, scratch)
            .is_some()
            || self
                .zero_apparent(vertices, simplex, dim, Pairing::Facet, scratch)
                .is_some()
    }
}

/// The working-column construction of 0.5.0, kept verbatim as the reference
/// the shipped construction is tested against. It sifts each cofacet into the
/// heap as it arrives. Do not change it: the tests require the shipped
/// construction to pop the same entries in the same order.
#[cfg(test)]
impl<C: Coeffs + Sync, D: Distances + Sync> Engine<'_, C, D> {
    /// The facet diameter of 0.6, kept as the reference the table-based
    /// maxima are tested against. It reads every pair of the facet.
    fn simplex_diameter_reference(&self, vertices: &[usize]) -> f64 {
        let mut d = 0.0f64;
        for (a, &va) in vertices.iter().enumerate() {
            for &vb in &vertices[a + 1..] {
                d = d.max(self.dist.get(va, vb));
            }
        }
        d
    }

    /// The zero-pivot facet search of 0.6, kept verbatim as the reference
    /// the shipped search is tested against. It takes the facet index from
    /// `FacetIter`, which searches for the vertex it drops, and the facet
    /// diameter from the distance source. Do not change it.
    pub(crate) fn zero_pivot_facet_reference(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
        facet_verts: &mut Vec<usize>,
    ) -> Option<(Simplex, usize, usize)> {
        let iter = FacetIter::new(&self.bt, simplex.index, dim, self.n);
        for (index, removed, k) in iter {
            facet_verts.clear();
            facet_verts.extend(vertices.iter().copied().filter(|&v| v != removed));
            let d = self.simplex_diameter_reference(facet_verts);
            if d == simplex.diameter {
                return Some((Simplex { diameter: d, index }, k, removed));
            }
        }
        None
    }

    /// The zero-apparent classifier of 0.6, kept as the reference the
    /// shipped classifier is tested against. It searches the facets with
    /// [`Engine::zero_pivot_facet_reference`].
    pub(crate) fn zero_apparent_reference(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
        pairing: Pairing,
        scratch: &mut PairScratch,
    ) -> Option<ApparentPair> {
        let PairScratch { facet, cofacet, .. } = scratch;
        match pairing {
            Pairing::Facet => {
                let (f, k, _) = self.zero_pivot_facet_reference(vertices, simplex, dim, facet)?;
                let (back, _, _) = self.zero_pivot_cofacet_with(facet, f, dim - 1)?;
                (back.index == simplex.index).then_some(ApparentPair { other: f, k })
            }
            Pairing::Cofacet => {
                let (c, k, added) = self.zero_pivot_cofacet_with(vertices, simplex, dim)?;
                insert_vertex(cofacet, vertices, added, k);
                let (back, _, _) = self.zero_pivot_facet_reference(cofacet, c, dim + 1, facet)?;
                (back.index == simplex.index).then_some(ApparentPair { other: c, k })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn init_coboundary_reference(
        &self,
        column: Simplex,
        dim: usize,
        has_pivot: impl Fn(u64) -> bool,
        working_cob: &mut BinaryHeap<HeapEntry>,
        cofacet_buf: &mut Vec<Entry>,
        verts: &mut Vec<usize>,
        cofacet_verts: &mut Vec<usize>,
        pairs: &mut PairScratch,
    ) -> Option<Entry> {
        self.bt.unrank(column.index, dim, self.n, verts);
        cofacet_buf.clear();
        let mut check_emergent = self.params.use_emergent_pairs;
        let emergent = self
            .dist
            .for_each_cofacet(&self.bt, column, verts, dim, false, |cf| {
                if !self.in_complex(cf.diameter) {
                    return ControlFlow::Continue(());
                }
                let cofacet = self.ops.pack(cf.diameter, cf.index, self.ops.sign(cf.k));
                cofacet_buf.push(cofacet);
                if check_emergent && cf.diameter == column.diameter {
                    if !has_pivot(cf.index) {
                        let stolen = self.params.use_apparent_pairs && {
                            insert_vertex(cofacet_verts, verts, cf.vertex, cf.k);
                            self.zero_apparent(
                                cofacet_verts,
                                self.ops.simplex(cofacet),
                                dim + 1,
                                Pairing::Facet,
                                pairs,
                            )
                            .is_some()
                        };
                        if !stolen {
                            return ControlFlow::Break(cofacet);
                        }
                    }
                    check_emergent = false;
                }
                ControlFlow::Continue(())
            });
        if let Some(p) = emergent {
            return Some(p);
        }
        for c in cofacet_buf.iter() {
            working_cob.push(HeapEntry::new(*c));
        }
        self.get_pivot(working_cob)
    }

    pub(crate) fn build_full_coboundary_reference(
        &self,
        column: Simplex,
        dim: usize,
        working_cob: &mut BinaryHeap<HeapEntry>,
        verts: &mut Vec<usize>,
    ) {
        self.bt.unrank(column.index, dim, self.n, verts);
        self.dist.for_each_cofacet_bounded(
            &self.bt,
            column,
            verts,
            dim,
            false,
            self.effective_threshold,
            |cf| {
                working_cob.push(HeapEntry::new(self.ops.pack(
                    cf.diameter,
                    cf.index,
                    self.ops.sign(cf.k),
                )));
                ControlFlow::<()>::Continue(())
            },
        );
    }
}

/// Sort key for one dim-0 edge: the diameter's bits above the complemented
/// index. A dim-0 edge's diameter is finite and not negative, so its bits
/// order as a `u64` exactly as `f64::total_cmp` orders the value, and the
/// complement turns ascending index into descending. Sorting the keys then
/// costs one `u128` comparison where the two-field comparator cost a
/// normalized float compare and an integer compare.
#[inline]
fn edge_key(e: Simplex) -> u128 {
    debug_assert!(e.diameter.is_finite() && e.diameter.is_sign_positive());
    ((e.diameter.to_bits() as u128) << 64) | (!e.index) as u128
}

/// The edge a key came from. Inverse of [`edge_key`].
#[inline]
fn edge_from_key(key: u128) -> Simplex {
    Simplex {
        diameter: f64::from_bits((key >> 64) as u64),
        index: !(key as u64),
    }
}

/// Write `vertices` with `added` inserted at position `at` into `out`. The
/// caller passes the enumerator's own position, so the insertion costs no
/// search.
#[inline]
fn insert_vertex(out: &mut Vec<usize>, vertices: &[usize], added: usize, at: usize) {
    debug_assert_eq!(at, vertices.partition_point(|&v| v < added));
    out.clear();
    out.reserve(vertices.len() + 1);
    out.extend_from_slice(&vertices[..at]);
    out.push(added);
    out.extend_from_slice(&vertices[at..]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::field::{counters, Fp, Z2};
    use crate::{DistanceMatrix, SparseDistanceMatrix};

    struct Rng(u64);
    impl Rng {
        fn new(seed: u64) -> Self {
            Rng(seed | 1)
        }
        fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn unit(&mut self) -> f64 {
            (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
        }
    }

    fn cloud(rng: &mut Rng, n: usize, coord_dim: usize) -> DistanceMatrix {
        let points: Vec<Vec<f64>> = (0..n)
            .map(|_| (0..coord_dim).map(|_| rng.unit()).collect())
            .collect();
        DistanceMatrix::from_points(&points).unwrap()
    }

    /// A lattice, where distances repeat and the heap sees many ties.
    fn lattice(side: usize) -> DistanceMatrix {
        let mut points = Vec::new();
        for i in 0..side {
            for j in 0..side {
                points.push(vec![i as f64, j as f64]);
            }
        }
        DistanceMatrix::from_points(&points).unwrap()
    }

    /// One heap entry as the bits that must not move.
    type Bits = (u64, u64);

    fn bits(e: Entry) -> Bits {
        (e.diameter.to_bits(), e.payload)
    }

    /// What one construction of a working column did.
    #[derive(Default)]
    struct Built {
        pivot: Option<Bits>,
        popped: Vec<Bits>,
        entries: usize,
        capacity: usize,
        comparisons: u64,
    }

    /// Totals over a whole run, for the mechanism report.
    #[derive(Default)]
    struct Totals {
        entries: usize,
        capacity: usize,
        comparisons: u64,
    }

    impl Totals {
        fn add(&mut self, b: &Built) {
            self.entries += b.entries;
            self.capacity = self.capacity.max(b.capacity);
            self.comparisons += b.comparisons;
        }
    }

    /// Drain `heap` with the field's cancellation and record what it gave.
    fn drain<C: Coeffs, D: Distances>(
        engine: &Engine<'_, C, D>,
        heap: &mut BinaryHeap<HeapEntry>,
        pivot: Option<Entry>,
        built: &mut Built,
    ) {
        built.pivot = pivot.map(bits);
        built.entries = heap.len();
        built.capacity = heap.capacity();
        while let Some(e) = engine.ops.pop_pivot(heap) {
            built.popped.push(bits(e));
        }
    }

    /// A graph with a small distance palette, so many triangles tie their
    /// edges; `keep` of four pairs get an edge, the rest are absent.
    fn tie_graph(rng: &mut Rng, n: usize, keep: u64, zero: bool) -> DistanceMatrix {
        let palette: &[f64] = if zero {
            &[0.0, 0.0, 1.0, 1.0]
        } else {
            &[0.5, 1.0, 1.0, 1.5, 2.0, 2.0, 3.0]
        };
        let mut condensed = Vec::new();
        for _ in 0..n * (n - 1) / 2 {
            condensed.push(if rng.next_u64() % 4 < keep {
                palette[(rng.next_u64() % palette.len() as u64) as usize]
            } else {
                f64::INFINITY
            });
        }
        DistanceMatrix::from_condensed(condensed).unwrap()
    }

    /// The row test against the neighbor-list classifier, edge by edge: on
    /// every cycle edge of a graph, with the rows activated exactly (as the
    /// serial walk does) and over-activated through the whole graph (as the
    /// parallel walk may), the two must agree. Covers `w` above, between,
    /// and below the edge's ends, zero distances, all-equal graphs,
    /// absent edges, and disconnected graphs.
    #[test]
    fn rows_agree_with_the_classifier_on_every_cycle_edge() {
        let mut rng = Rng::new(0x51ed_270e_c9c0_ffee);
        let mut tested = 0usize;
        let mut params = RipsParams {
            threshold: Some(2.5),
            ..RipsParams::default()
        };
        for case in 0..40 {
            let n = 8 + case % 23;
            let dist = tie_graph(&mut rng, n, 1 + case as u64 % 3, case % 5 == 0);
            let threshold = if case % 7 == 0 { f64::MAX } else { 2.5 };
            params.threshold = Some(threshold);
            let engine = Engine::new(&dist, &params, Z2).unwrap();
            let Some(adjacency) = crate::adjacency::Adjacency::build_gated(&dist, threshold, 0)
            else {
                continue;
            };
            let mut sorted = engine.edges();
            sorted.sort_unstable_by(|a, b| {
                a.diameter
                    .total_cmp(&b.diameter)
                    .then(b.index.cmp(&a.index))
            });
            let mut full = crate::adjacency::Rows::new(n);
            let mut verts = Vec::new();
            for e in &sorted {
                engine.bt.unrank(e.index, 1, n, &mut verts);
                full.set(verts[0], verts[1]);
            }
            let mut exact = crate::adjacency::Rows::new(n);
            let mut uf = UnionFind::new(n);
            let mut pairs = PairScratch::default();
            let mut group = 0;
            while group < sorted.len() {
                let d = sorted[group].diameter;
                let mut end = group;
                while end < sorted.len() && sorted[end].diameter == d {
                    engine.bt.unrank(sorted[end].index, 1, n, &mut verts);
                    exact.set(verts[0], verts[1]);
                    end += 1;
                }
                for e in &sorted[group..end] {
                    engine.bt.unrank(e.index, 1, n, &mut verts);
                    let (ru, rv) = (uf.find(verts[0]), uf.find(verts[1]));
                    if ru != rv {
                        uf.link(ru, rv);
                        continue;
                    }
                    let classifier = engine
                        .zero_apparent(&verts, *e, 1, Pairing::Cofacet, &mut pairs)
                        .is_some();
                    let by_exact = exact.pairs_edge(&adjacency, verts[0], verts[1], d);
                    let by_full = full.pairs_edge(&adjacency, verts[0], verts[1], d);
                    assert_eq!(
                        by_exact, classifier,
                        "case {case} edge {:?} exact rows",
                        verts
                    );
                    assert_eq!(
                        by_full, classifier,
                        "case {case} edge {:?} full rows",
                        verts
                    );
                    tested += 1;
                }
                group = end;
            }
        }
        assert!(tested > 2000, "{tested} cycle edges tested");
    }

    /// Build every column's working coboundary both ways and require the
    /// same pivot and the same drained sequence. Returns the reference and
    /// the shipped totals.
    fn heaps_agree<C: Coeffs + Sync>(
        label: &str,
        dist: &DistanceMatrix,
        params: &RipsParams,
        ops: C,
    ) -> (Totals, Totals) {
        let engine = Engine::new(dist, params, ops).unwrap();
        let mut diagram = Diagram::default();
        let edges = engine.edges();
        let mut columns = engine.dim0_pairs(&edges, &mut diagram);
        let mut simplices = edges;
        let mut prev_pivots: Pivots = FxHashMap::default();
        let mut reference = Totals::default();
        let mut shipped = Totals::default();

        let mut heap_a: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut heap_b: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut buf_a: Vec<Entry> = Vec::new();
        let mut buf_b: Vec<Entry> = Vec::new();
        let mut verts = Vec::new();
        let mut cofacet_verts = Vec::new();
        let mut pairs = PairScratch::default();

        for dim in 1..=engine.max_dim {
            for &column in &columns {
                // The pivot map decides whether the emergent shortcut can
                // fire, so both answers are exercised. Under `true` no
                // shortcut fires and every column builds its heap.
                for claimed in [false, true] {
                    let mut a = Built::default();
                    let mut b = Built::default();
                    heap_a.clear();
                    counters::reset();
                    let pivot = engine.init_coboundary_reference(
                        column,
                        dim,
                        |_| claimed,
                        &mut heap_a,
                        &mut buf_a,
                        &mut verts,
                        &mut cofacet_verts,
                        &mut pairs,
                    );
                    a.comparisons = counters::comparisons();
                    drain(&engine, &mut heap_a, pivot, &mut a);

                    heap_b.clear();
                    counters::reset();
                    let pivot = engine.init_coboundary(
                        column,
                        dim,
                        |_| claimed,
                        &mut heap_b,
                        &mut buf_b,
                        &mut verts,
                        &mut cofacet_verts,
                        &mut pairs,
                    );
                    b.comparisons = counters::comparisons();
                    drain(&engine, &mut heap_b, pivot, &mut b);

                    assert_eq!(a.pivot, b.pivot, "{label}: init pivot, dim {dim}");
                    assert_eq!(a.popped, b.popped, "{label}: init pops, dim {dim}");
                    assert_eq!(a.entries, b.entries, "{label}: init entries, dim {dim}");
                    reference.add(&a);
                    shipped.add(&b);

                    let mut a = Built::default();
                    let mut b = Built::default();
                    heap_a.clear();
                    counters::reset();
                    engine.build_full_coboundary_reference(column, dim, &mut heap_a, &mut verts);
                    a.comparisons = counters::comparisons();
                    drain(&engine, &mut heap_a, None, &mut a);
                    heap_b.clear();
                    counters::reset();
                    engine.build_full_coboundary(column, dim, &mut heap_b, &mut verts);
                    b.comparisons = counters::comparisons();
                    drain(&engine, &mut heap_b, None, &mut b);
                    assert_eq!(a.popped, b.popped, "{label}: full pops, dim {dim}");
                    reference.add(&a);
                    shipped.add(&b);
                }
            }
            let pivots = engine.reduce_dimension(&columns, dim, &prev_pivots, &mut diagram);
            if dim < engine.max_dim {
                (simplices, columns) =
                    engine.assemble(&simplices, dim + 1, &pivots, dim + 1 < engine.max_dim);
            }
            prev_pivots = pivots;
        }
        (reference, shipped)
    }

    /// The bulk heapify must give the pivot and the whole cancelled pop
    /// sequence the sift-up loop gives, over Z/2 and over odd primes, with
    /// the emergent shortcut on and off.
    #[test]
    fn the_bulk_heapify_pops_the_same_entries() {
        let mut rng = Rng::new(0x8eaa_1f70_0000_0001);
        let inputs = [
            ("cube", cloud(&mut rng, 40, 3)),
            ("plane", cloud(&mut rng, 30, 2)),
            ("lattice", lattice(5)),
        ];
        let mut entries = 0usize;
        for (label, dist) in &inputs {
            for max_dim in [1usize, 2] {
                for emergent in [true, false] {
                    let mut params = RipsParams::new(max_dim);
                    params.use_emergent_pairs = emergent;
                    // The engine takes its field directly, so the modulus
                    // here picks the arithmetic and not a parameter.
                    for modulus in [2u64, 3, 5] {
                        let (a, b) = if modulus == 2 {
                            heaps_agree(label, dist, &params, Z2)
                        } else {
                            heaps_agree(label, dist, &params, Fp::new(modulus))
                        };
                        assert_eq!(a.entries, b.entries, "{label}: heap entries");
                        entries += b.entries;
                    }
                }
            }
        }
        assert!(entries > 0, "no working column was built");
    }

    /// A sparse matrix that keeps the pairs at or below `keep` of a cloud,
    /// so the sparse `get` and the sparse enumerator run in the test.
    fn sparse_cloud(rng: &mut Rng, n: usize, coord_dim: usize, keep: f64) -> SparseDistanceMatrix {
        let dense = cloud(rng, n, coord_dim);
        let mut triplets = Vec::new();
        for i in 1..n {
            for j in 0..i {
                let d = dense.get(i, j);
                if d <= keep {
                    triplets.push((i, j, d));
                }
            }
        }
        SparseDistanceMatrix::from_triplets(n, &triplets).unwrap()
    }

    /// One apparent-pair answer as the bits that must not move.
    fn pair_bits(p: Option<ApparentPair>) -> Option<(u64, u64, usize)> {
        p.map(|p| (p.other.diameter.to_bits(), p.other.index, p.k))
    }

    /// Query both facet searches on the same simplex and require the same
    /// answer, down to the facet's vertices. Returns the number of facet
    /// searches that found a facet.
    fn facet_searches_agree<C: Coeffs + Sync, D: Distances + Sync>(
        label: &str,
        engine: &Engine<'_, C, D>,
        simplices: &[Simplex],
        dim: usize,
    ) -> usize {
        let mut verts = Vec::new();
        let mut shipped_verts = Vec::new();
        let mut reference_verts = Vec::new();
        let mut found = 0usize;
        for &s in simplices {
            engine.bt.unrank(s.index, dim, engine.n, &mut verts);
            // A fresh table each time, so the search fills it from cold.
            let mut table = PairTable::default();
            let shipped = engine.zero_pivot_facet_with(&verts, s, &mut table, &mut shipped_verts);
            let reference = engine.zero_pivot_facet_reference(&verts, s, dim, &mut reference_verts);
            let key = |r: Option<(Simplex, usize, usize)>| {
                r.map(|(f, k, removed)| (f.diameter.to_bits(), f.index, k, removed))
            };
            assert_eq!(
                key(shipped),
                key(reference),
                "{label}: facet search, dim {dim}"
            );
            if shipped.is_some() {
                assert_eq!(
                    shipped_verts, reference_verts,
                    "{label}: facet vertices, dim {dim}"
                );
                found += 1;
            }
        }
        found
    }

    /// Classify every simplex both ways and in both directions, and require
    /// the same answer. Returns the number of classifications made.
    fn classifiers_agree<C: Coeffs + Sync, D: Distances + Sync>(
        label: &str,
        engine: &Engine<'_, C, D>,
        simplices: &[Simplex],
        dim: usize,
    ) -> usize {
        let mut verts = Vec::new();
        let mut shipped = PairScratch::default();
        let mut reference = PairScratch::default();
        let mut tested = 0usize;
        for &s in simplices {
            engine.bt.unrank(s.index, dim, engine.n, &mut verts);
            for pairing in [Pairing::Cofacet, Pairing::Facet] {
                if pairing == Pairing::Facet && dim == 0 {
                    continue;
                }
                let a = engine.zero_apparent(&verts, s, dim, pairing, &mut shipped);
                let b = engine.zero_apparent_reference(&verts, s, dim, pairing, &mut reference);
                assert_eq!(pair_bits(a), pair_bits(b), "{label}: classifier, dim {dim}");
                tested += 1;
            }
        }
        tested
    }

    /// Walk the complex of `dist` and check both kernels at every dimension.
    fn kernels_agree<D: Distances + Sync>(label: &str, dist: &D, max_dim: usize) -> (usize, usize) {
        let params = RipsParams::new(max_dim);
        let engine = Engine::new(dist, &params, Z2).unwrap();
        let mut diagram = Diagram::default();
        let edges = engine.edges();
        let mut columns = engine.dim0_pairs(&edges, &mut diagram);
        let mut simplices = edges;
        let (mut found, mut tested) = (0usize, 0usize);
        for dim in 1..=engine.max_dim {
            found += facet_searches_agree(label, &engine, &simplices, dim);
            tested += classifiers_agree(label, &engine, &simplices, dim);
            let pivots = engine.reduce_dimension(&columns, dim, &Pivots::default(), &mut diagram);
            if dim < engine.max_dim {
                (simplices, columns) =
                    engine.assemble(&simplices, dim + 1, &pivots, dim + 1 < engine.max_dim);
            }
        }
        (found, tested)
    }

    /// The table-based facet diameters and the arithmetic facet index must
    /// give what the search-and-read path gives, bit for bit, on dense and
    /// sparse sources and on input with many tied distances.
    #[test]
    fn the_facet_kernels_agree_with_the_reference() {
        let mut rng = Rng::new(0x8eaa_1f70_0000_0003);
        let (mut found, mut tested) = (0usize, 0usize);
        for (label, dist, max_dim) in [
            ("cube", cloud(&mut rng, 36, 3), 3),
            ("plane", cloud(&mut rng, 30, 2), 2),
            ("lattice", lattice(5), 2),
        ] {
            let (f, t) = kernels_agree(label, &dist, max_dim);
            found += f;
            tested += t;
        }
        for (label, keep, max_dim) in [("sparse-near", 0.35f64, 3), ("sparse-wide", 0.8, 2)] {
            let dist = sparse_cloud(&mut rng, 40, 3, keep);
            let (f, t) = kernels_agree(label, &dist, max_dim);
            found += f;
            tested += t;
        }
        assert!(found > 0, "no facet search found a facet");
        assert!(tested > 0, "no simplex was classified");
    }

    /// The key sort must put the dim-0 edges where the two-field comparator
    /// put them, on dense and sparse sources.
    #[test]
    fn the_edge_key_sort_matches_the_comparator() {
        let mut rng = Rng::new(0x8eaa_1f70_0000_0004);
        let order = |a: &Simplex, b: &Simplex| {
            a.diameter
                .total_cmp(&b.diameter)
                .then(b.index.cmp(&a.index))
        };
        let dense = [cloud(&mut rng, 120, 3), lattice(9)];
        let sparse = [sparse_cloud(&mut rng, 150, 3, 0.4)];
        let mut edge_sets: Vec<Vec<Simplex>> = Vec::new();
        let params = RipsParams::new(1);
        for dist in &dense {
            edge_sets.push(Engine::new(dist, &params, Z2).unwrap().edges());
        }
        for dist in &sparse {
            edge_sets.push(Engine::new(dist, &params, Z2).unwrap().edges());
        }
        for edges in &edge_sets {
            assert!(!edges.is_empty(), "no edge to sort");
            let mut want = edges.clone();
            want.sort_unstable_by(order);
            let mut keys: Vec<u128> = edges.iter().map(|&e| edge_key(e)).collect();
            keys.sort_unstable();
            let got: Vec<Simplex> = keys.into_iter().map(edge_from_key).collect();
            assert_eq!(want.len(), got.len());
            for (w, g) in want.iter().zip(&got) {
                assert_eq!(
                    (w.diameter.to_bits(), w.index),
                    (g.diameter.to_bits(), g.index),
                    "key sort order"
                );
            }
        }
    }

    /// What the construction costs in counters: entries, comparisons, and
    /// the largest heap capacity. Not a gate.
    #[test]
    #[ignore]
    fn heap_construction_counters() {
        let mut rng = Rng::new(0x8eaa_1f70_0000_0002);
        for (label, dist, max_dim) in [
            ("cube-d1", cloud(&mut rng, 200, 3), 1),
            ("cube-d2", cloud(&mut rng, 90, 3), 2),
            ("lattice-d1", lattice(12), 1),
        ] {
            let params = RipsParams::new(max_dim);
            let (a, b) = heaps_agree(label, &dist, &params, Z2);
            println!(
                "HEAP {label} entries={} comparisons: pushes={} heapify={} ratio={:.3} \
                 capacity: pushes={} heapify={}",
                b.entries,
                a.comparisons,
                b.comparisons,
                b.comparisons as f64 / a.comparisons as f64,
                a.capacity,
                b.capacity
            );
        }
    }
}
