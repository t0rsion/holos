use std::ops::ControlFlow;

use crate::distances::Distances;
use crate::field::Coeffs;
use crate::simplex::Simplex;

use super::{ApparentPair, Engine, PairScratch, PairTable, Pairing};

impl<'a, C: Coeffs + Sync, D: Distances + Sync> Engine<'a, C, D> {
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
    pub(super) fn zero_pivot_facet_with(
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
    pub(super) fn zero_pivot_cofacet_with(
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
    pub(super) fn is_in_zero_apparent_pair(
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
/// Write `vertices` with `added` inserted at position `at` into `out`. The
/// caller passes the enumerator's own position, so the insertion costs no
/// search.
#[inline]
pub(super) fn insert_vertex(out: &mut Vec<usize>, vertices: &[usize], added: usize, at: usize) {
    debug_assert_eq!(at, vertices.partition_point(|&v| v < added));
    out.clear();
    out.reserve(vertices.len() + 1);
    out.extend_from_slice(&vertices[..at]);
    out.push(added);
    out.extend_from_slice(&vertices[at..]);
}
