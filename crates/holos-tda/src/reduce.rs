//! The serial reduction engine: implicit persistent cohomology following
//! ripser (Bauer 2021). The engine uses the anti-transpose convention,
//! clearing, emergent and apparent pair shortcuts, lazy-cancellation heaps,
//! and on-demand regeneration of reducer columns. Simplices exist only as
//! combinadic indices. The engine never materializes a simplex list of
//! dimension max_dim+1.
//!
//! Every method takes `&self`: the engine holds no mutable scratch, so the
//! same cores drive both this serial path and the parallel one in
//! [`crate::parallel`].

use std::collections::BinaryHeap;
use std::ops::ControlFlow;

use rayon::prelude::*;
use rustc_hash::FxHashMap;

use crate::combinadic::{BinomialTable, FacetIter};
use crate::distances::Distances;
use crate::field::{Coeffs, Entry, HeapEntry};
use crate::simplex::Simplex;
use crate::union_find::UnionFind;
use crate::{Bar, Diagram, Result, RipsParams};

/// The pivot registry for one dimension: pivot coefficient and column
/// position, keyed by pivot index. Consumed as the clearing set of the next
/// dimension.
pub(crate) type Pivots = FxHashMap<u64, (u64, usize)>;

/// Below this many simplices, assembly stays serial: the parallel split costs
/// more than it saves.
const PARALLEL_ASSEMBLE_MIN: usize = 4096;

pub(crate) struct Engine<'a, C: Coeffs, D: Distances> {
    pub(crate) dist: &'a D,
    pub(crate) bt: BinomialTable,
    pub(crate) n: usize,
    pub(crate) threshold: f64,
    pub(crate) max_dim: usize,
    pub(crate) params: &'a RipsParams,
    pub(crate) ops: C,
    /// One worker pool for the whole run. Every parallel region installs onto
    /// it, so the pool is built once and the thread count is honored exactly.
    /// `None` runs serially.
    pool: Option<rayon::ThreadPool>,
}

impl<'a, C: Coeffs + Sync, D: Distances + Sync> Engine<'a, C, D> {
    pub(crate) fn new(dist: &'a D, params: &'a RipsParams, ops: C) -> Result<Self> {
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
        Ok(Self {
            dist,
            bt,
            n,
            threshold,
            max_dim,
            params,
            ops,
            pool,
        })
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
            let pivots = if self.params.threads > 1 {
                let (pivots, bars) = self.reduce_dimension_parallel(&columns, dim, &prev_pivots);
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
        diameter <= self.threshold && diameter.is_finite()
    }

    fn edges(&self) -> Vec<Simplex> {
        let mut edges = Vec::new();
        let bt = &self.bt;
        let threshold = self.threshold;
        self.dist.for_each_edge(&mut |i, j, d| {
            if d <= threshold && d.is_finite() {
                edges.push(Simplex {
                    diameter: d,
                    index: bt.get(i, 2) + j as u64,
                });
            }
        });
        edges
    }

    /// Union-find pass: emits dim-0 bars and returns the dim-1 columns
    /// (cycle edges), sorted for reduction (diameter descending, index
    /// ascending).
    fn dim0_pairs(&self, edges: &[Simplex], diagram: &mut Diagram) -> Vec<Simplex> {
        let mut sorted: Vec<Simplex> = edges.to_vec();
        let order = |a: &Simplex, b: &Simplex| {
            a.diameter
                .total_cmp(&b.diameter)
                .then(b.index.cmp(&a.index))
        };
        // Unique (diameter, index) keys, so the unstable sort is deterministic
        // and the parallel and serial orders agree.
        match &self.pool {
            Some(pool) => pool.install(|| sorted.par_sort_unstable_by(order)),
            None => sorted.sort_unstable_by(order),
        }
        let mut uf = UnionFind::new(self.n);
        let mut columns = Vec::new();
        let mut verts = Vec::new();
        for e in &sorted {
            self.bt.unrank(e.index, 1, self.n, &mut verts);
            let (ru, rv) = (uf.find(verts[0]), uf.find(verts[1]));
            if ru != rv {
                if e.diameter > 0.0 {
                    diagram.bars.push(Bar {
                        dim: 0,
                        birth: 0.0,
                        death: e.diameter,
                    });
                }
                uf.link(ru, rv);
            } else if self.max_dim > 0
                && (!self.params.use_apparent_pairs || self.zero_apparent_cofacet(*e, 1).is_none())
            {
                columns.push(*e);
            }
        }
        for v in 0..self.n {
            if uf.find(v) == v {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death: f64::INFINITY,
                });
            }
        }
        columns.reverse();
        columns
    }

    /// Canonical cofacet assembly with clearing and apparent-pair pruning.
    /// Returns (all (d)-simplices, columns to reduce in dimension d). The
    /// simplex list seeds the next dimension's assembly. The final dimension
    /// has no such consumer, so `seed_next` is false there.
    ///
    /// Generation is per-simplex independent, so it runs in parallel over
    /// chunks. Concatenating in chunk order reproduces the serial output
    /// exactly, and the sort then fixes the column order.
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

        if self.params.threads <= 1 || simplices.len() < PARALLEL_ASSEMBLE_MIN {
            let (next_simplices, mut columns) =
                self.assemble_chunk(simplices, dim, prev_pivots, seed_next);
            columns.sort_by(column_order);
            return (next_simplices, columns);
        }

        let chunk = (simplices.len() / (self.params.threads * 4)).clamp(64, 4096);
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
            columns.par_sort_unstable_by(column_order);
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
        for s in simplices {
            self.bt.unrank(s.index, dim - 1, self.n, &mut verts);
            self.dist
                .for_each_cofacet(&self.bt, *s, &verts, dim - 1, true, |cf| {
                    if self.in_complex(cf.diameter) {
                        let cofacet = Simplex {
                            diameter: cf.diameter,
                            index: cf.index,
                        };
                        if seed_next {
                            next_simplices.push(cofacet);
                        }
                        let cleared =
                            self.params.use_clearing && prev_pivots.contains_key(&cf.index);
                        // Only pay for the apparent-pair test when the cheaper
                        // clearing check has not already excluded the cofacet.
                        if !cleared {
                            let apparent = self.params.use_apparent_pairs
                                && self.is_in_zero_apparent_pair(cofacet, dim);
                            if !apparent {
                                columns.push(cofacet);
                            }
                        }
                    }
                    ControlFlow::<()>::Continue(())
                });
        }
        (next_simplices, columns)
    }

    /// Reduce the dim-d columns serially against implicit (d+1)-rows.
    pub(crate) fn reduce_dimension(
        &self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        diagram: &mut Diagram,
    ) -> Pivots {
        let mut pivot_map: Pivots = FxHashMap::default();
        let mut v_entries: Vec<Entry> = Vec::new();
        let mut v_offsets: Vec<usize> = vec![0];
        let mut working_cob: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut working_red: BinaryHeap<HeapEntry> = BinaryHeap::new();
        let mut cofacet_buf: Vec<Entry> = Vec::new();
        let mut verts: Vec<usize> = Vec::new();

        for (col_pos, &column) in columns.iter().enumerate() {
            working_cob.clear();
            working_red.clear();
            let mut pivot = self.init_coboundary(
                column,
                dim,
                |index| pivot_map.contains_key(&index),
                &mut working_cob,
                &mut cofacet_buf,
                &mut verts,
            );
            loop {
                match pivot {
                    Some(p) => {
                        let p_index = self.ops.index(p);
                        if let Some(&(other_coeff, other_pos)) = pivot_map.get(&p_index) {
                            let (lo, hi) = (v_offsets[other_pos], v_offsets[other_pos + 1]);
                            self.fold_reducer(
                                p,
                                other_coeff,
                                columns[other_pos],
                                &v_entries[lo..hi],
                                dim,
                                &mut working_red,
                                &mut working_cob,
                                &mut verts,
                            );
                            pivot = self.get_pivot(&mut working_cob);
                        } else if let Some(apparent) = self.reduce_apparent_facet(
                            p,
                            dim,
                            &mut working_red,
                            &mut working_cob,
                            &mut verts,
                        ) {
                            pivot = apparent;
                        } else {
                            self.emit_pair(column, self.ops.simplex(p), dim, diagram);
                            pivot_map.insert(p_index, (self.ops.coeff(p), col_pos));
                            self.drain_into(&mut working_red, &mut v_entries);
                            break;
                        }
                    }
                    None => {
                        // With clearing disabled, columns that were pivots of
                        // the previous dimension reduce to zero here. They are
                        // deaths, not essential classes.
                        let is_prior_death =
                            !self.params.use_clearing && prev_pivots.contains_key(&column.index);
                        if !is_prior_death {
                            diagram.bars.push(Bar {
                                dim,
                                birth: column.diameter,
                                death: f64::INFINITY,
                            });
                        }
                        break;
                    }
                }
            }
            v_offsets.push(v_entries.len());
        }
        pivot_map
    }

    /// Record the finite bar of a birth/death pair. A zero-persistence pair
    /// (`death == birth`) emits no bar.
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
    pub(crate) fn reduce_apparent_facet(
        &self,
        p: Entry,
        dim: usize,
        working_red: &mut BinaryHeap<HeapEntry>,
        working_cob: &mut BinaryHeap<HeapEntry>,
        verts: &mut Vec<usize>,
    ) -> Option<Option<Entry>> {
        if !self.params.use_apparent_pairs {
            return None;
        }
        let (facet, k) = self.zero_apparent_facet(self.ops.simplex(p), dim + 1)?;
        // Ripser negates the facet's boundary coefficient so the pivot cancels
        // exactly.
        let coeff = self
            .ops
            .neg(self.ops.mul(self.ops.sign(k), self.ops.coeff(p)));
        let e = self.ops.pack(facet.diameter, facet.index, coeff);
        self.add_simplex_coboundary(e, dim, working_red, working_cob, verts);
        Some(self.get_pivot(working_cob))
    }

    /// Ripser's init_coboundary_and_get_pivot: enumerate the coboundary and
    /// return its pivot. When the emergent shortcut fires, the pivot comes
    /// back without building the working column. `has_pivot` answers whether
    /// a given cofacet index is already a claimed pivot.
    pub(crate) fn init_coboundary(
        &self,
        column: Simplex,
        dim: usize,
        has_pivot: impl Fn(u64) -> bool,
        working_cob: &mut BinaryHeap<HeapEntry>,
        cofacet_buf: &mut Vec<Entry>,
        verts: &mut Vec<usize>,
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
                        let stolen = self.params.use_apparent_pairs
                            && self
                                .zero_apparent_facet(self.ops.simplex(cofacet), dim + 1)
                                .is_some();
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
            working_cob.push(HeapEntry(*c));
        }
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
        self.dist
            .for_each_cofacet(&self.bt, column, verts, dim, false, |cf| {
                if self.in_complex(cf.diameter) {
                    working_cob.push(HeapEntry(self.ops.pack(
                        cf.diameter,
                        cf.index,
                        self.ops.sign(cf.k),
                    )));
                }
                ControlFlow::<()>::Continue(())
            });
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
        working_red.push(HeapEntry(entry));
        self.bt.unrank(self.ops.index(entry), dim, self.n, verts);
        let ops = &self.ops;
        let entry_coeff = ops.coeff(entry);
        let threshold = self.threshold;
        self.dist
            .for_each_cofacet(&self.bt, ops.simplex(entry), verts, dim, false, |cf| {
                if cf.diameter <= threshold && cf.diameter.is_finite() {
                    let coeff = ops.mul(ops.sign(cf.k), entry_coeff);
                    working_cob.push(HeapEntry(ops.pack(cf.diameter, cf.index, coeff)));
                }
                ControlFlow::<()>::Continue(())
            });
    }

    pub(crate) fn get_pivot(&self, heap: &mut BinaryHeap<HeapEntry>) -> Option<Entry> {
        let pivot = self.ops.pop_pivot(heap)?;
        heap.push(HeapEntry(pivot));
        Some(pivot)
    }

    /// Drain the working reduction column, cancelled in the field, into the
    /// V store.
    pub(crate) fn drain_into(&self, heap: &mut BinaryHeap<HeapEntry>, out: &mut Vec<Entry>) {
        while let Some(e) = self.ops.pop_pivot(heap) {
            out.push(e);
        }
    }

    fn simplex_diameter(&self, vertices: &[usize]) -> f64 {
        let mut d = 0.0f64;
        for (a, &va) in vertices.iter().enumerate() {
            for &vb in &vertices[a + 1..] {
                d = d.max(self.dist.get(va, vb));
            }
        }
        d
    }

    /// First facet (in ripser's facet order) with the same diameter, along
    /// with its enumerator position k (for the boundary sign).
    /// `vertices` must be the simplex's vertex set.
    fn zero_pivot_facet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<(Simplex, usize)> {
        let iter = FacetIter::new(&self.bt, simplex.index, dim, self.n);
        let mut facet_verts = Vec::with_capacity(vertices.len());
        for (index, removed, k) in iter {
            facet_verts.clear();
            facet_verts.extend(vertices.iter().copied().filter(|&v| v != removed));
            let d = self.simplex_diameter(&facet_verts);
            if d == simplex.diameter {
                return Some((Simplex { diameter: d, index }, k));
            }
        }
        None
    }

    /// First cofacet (descending index order) with the same diameter.
    fn zero_pivot_cofacet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<Simplex> {
        self.dist
            .for_each_cofacet(&self.bt, simplex, vertices, dim, false, |cf| {
                if cf.diameter == simplex.diameter {
                    ControlFlow::Break(Simplex {
                        diameter: cf.diameter,
                        index: cf.index,
                    })
                } else {
                    ControlFlow::Continue(())
                }
            })
    }

    fn zero_apparent_facet(&self, simplex: Simplex, dim: usize) -> Option<(Simplex, usize)> {
        let mut verts = Vec::new();
        self.bt.unrank(simplex.index, dim, self.n, &mut verts);
        self.zero_apparent_facet_with(&verts, simplex, dim)
    }

    fn zero_apparent_facet_with(
        &self,
        vertices: &[usize],
        simplex: Simplex,
        dim: usize,
    ) -> Option<(Simplex, usize)> {
        let (facet, k) = self.zero_pivot_facet_with(vertices, simplex, dim)?;
        let mut facet_verts = Vec::new();
        self.bt
            .unrank(facet.index, dim - 1, self.n, &mut facet_verts);
        match self.zero_pivot_cofacet_with(&facet_verts, facet, dim - 1) {
            Some(c) if c.index == simplex.index => Some((facet, k)),
            _ => None,
        }
    }

    fn zero_apparent_cofacet(&self, simplex: Simplex, dim: usize) -> Option<Simplex> {
        let mut verts = Vec::new();
        self.bt.unrank(simplex.index, dim, self.n, &mut verts);
        let cofacet = self.zero_pivot_cofacet_with(&verts, simplex, dim)?;
        let mut cverts = Vec::new();
        self.bt.unrank(cofacet.index, dim + 1, self.n, &mut cverts);
        match self.zero_pivot_facet_with(&cverts, cofacet, dim + 1) {
            Some((f, _)) if f.index == simplex.index => Some(cofacet),
            _ => None,
        }
    }

    fn is_in_zero_apparent_pair(&self, simplex: Simplex, dim: usize) -> bool {
        self.zero_apparent_cofacet(simplex, dim).is_some()
            || self.zero_apparent_facet(simplex, dim).is_some()
    }
}
