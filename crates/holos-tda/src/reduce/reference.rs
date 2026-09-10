use std::collections::BinaryHeap;
use std::ops::ControlFlow;

use crate::combinadic::FacetIter;
use crate::distances::Distances;
use crate::field::{Coeffs, Entry, HeapEntry};
use crate::simplex::Simplex;

use super::pairing::insert_vertex;
use super::{ApparentPair, Engine, PairScratch, Pairing};

/// The working-column construction of 0.5.0, kept verbatim as the reference
/// the shipped construction is tested against. It sifts each cofacet into the
/// heap as it arrives.
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
    /// diameter from the distance source.
    pub(super) fn zero_pivot_facet_reference(
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
    pub(super) fn zero_apparent_reference(
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
    pub(super) fn init_coboundary_reference(
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

    pub(super) fn build_full_coboundary_reference(
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
