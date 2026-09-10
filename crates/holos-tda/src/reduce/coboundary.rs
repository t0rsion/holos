use std::collections::BinaryHeap;
use std::ops::ControlFlow;

use crate::distances::Distances;
use crate::field::{Coeffs, Entry, HeapEntry};
use crate::simplex::Simplex;

use super::pairing::insert_vertex;
use super::{Engine, PairScratch, Pairing};

impl<'a, C: Coeffs + Sync, D: Distances + Sync> Engine<'a, C, D> {
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
    /// column. `working_cob` stays as the caller left it. A pivot beside an
    /// empty `working_cob` marks that case. `has_pivot` answers whether a
    /// given cofacet index is already a claimed pivot. A caller whose
    /// answer can go stale must test the pivot again itself.
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
        // Same heapify as [`Engine::init_coboundary`].
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
}
