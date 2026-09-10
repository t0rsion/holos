use std::ops::ControlFlow;

use crate::combinadic::{BinomialTable, CofacetIter};
use crate::simplex::Simplex;

pub(crate) mod counters;
mod dense;
#[cfg(test)]
mod reference;
mod sparse;

/// A cofacet produced during enumeration: its combinadic index, the position
/// `k` of the added vertex in the cofacet (the coboundary sign exponent), the
/// added vertex itself, and the cofacet's filtration diameter. The vertex
/// lets a caller build the cofacet's vertex set from the simplex's own set
/// instead of unranking the cofacet.
///
/// Under `upper_only` every enumerator reports `k` as 0, not as `dim + 1`.
/// No caller reads the position there.
pub(crate) struct Cofacet {
    pub(crate) index: u64,
    pub(crate) k: usize,
    pub(crate) vertex: usize,
    pub(crate) diameter: f64,
}

/// Cursor slots a sparse cofacet enumeration keeps on the stack. A simplex
/// wider than this allocates its cursors once, on entry.
pub(super) const INLINE_VERTS: usize = 16;

/// What the solver needs from a distance source. An absent pair reads as +inf.
pub(crate) trait Distances {
    fn len(&self) -> usize;
    fn get(&self, i: usize, j: usize) -> f64;
    /// Threshold to use when the caller gives none.
    fn default_threshold(&self) -> f64;
    /// The largest distance the source can report between distinct points,
    /// or +inf when it knows no such limit. A bound at or above it drops
    /// nothing, so [`Distances::for_each_cofacet_bounded`] raises it to
    /// infinity.
    fn max_distance(&self) -> f64 {
        f64::INFINITY
    }
    /// Visit every pair that could be an edge, as (i, j, d) with j < i.
    fn for_each_edge(&self, f: impl FnMut(usize, usize, f64));

    /// Enumerate cofacets of `simplex` (vertex set `verts`, ascending) in
    /// dimension `dim`, in strictly descending index order. `f` runs on each
    /// cofacet. With `upper_only`, restrict to cofacets whose added vertex
    /// exceeds every simplex vertex. Over all d-simplices, that restriction
    /// generates each (d+1)-simplex exactly once. Diameters may exceed the
    /// threshold or be infinite: the caller filters. `f` may short-circuit
    /// with `Break`.
    ///
    /// The default walks the full combinadic cofacet set. A sparse source
    /// visits only common neighbors.
    #[inline]
    fn for_each_cofacet<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        self.enumerate_cofacets::<false, T, _>(
            bt,
            simplex,
            verts,
            dim,
            upper_only,
            f64::INFINITY,
            f,
        )
    }

    /// [`Distances::for_each_cofacet`] restricted to the cofacets whose
    /// diameter is at or below `bound`. Those reach `f` in the same order and
    /// with the same bits as the unrestricted walk. `bound` must be at or
    /// above `simplex.diameter`.
    ///
    /// A cofacet diameter is the largest of the simplex diameter and the
    /// distances from the added vertex to the simplex vertices, so the fold
    /// can stop at the first distance above the bound.
    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn for_each_cofacet_bounded<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
        f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        debug_assert!(bound >= simplex.diameter);
        // A bound no distance reaches drops nothing, and infinity is such a
        // bound. Raising it there turns the test in the walk into one the
        // branch predictor always gets right, and it keeps one instance of
        // the walk at the call site.
        let mut bound = bound;
        if bound >= self.max_distance() {
            counters::note_vacuous_bound();
            bound = f64::INFINITY;
        }
        self.enumerate_cofacets::<true, T, _>(bt, simplex, verts, dim, upper_only, bound, f)
    }

    /// The one cofacet walk behind [`Distances::for_each_cofacet`] and
    /// [`Distances::for_each_cofacet_bounded`]. `BOUNDED` selects the bounded
    /// form, and only that form reads `bound`. A distance source overrides
    /// this method alone, so the two entry points cannot drift apart, and the
    /// unbounded one carries no bound test.
    #[allow(clippy::too_many_arguments)]
    fn enumerate_cofacets<const BOUNDED: bool, T, F>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
        mut f: F,
    ) -> Option<T>
    where
        F: FnMut(Cofacet) -> ControlFlow<T>,
    {
        let cofacet_diameter = |added: usize| {
            let mut d = simplex.diameter;
            for &v in verts {
                let x = self.get(added, v);
                if BOUNDED && x > bound {
                    return None;
                }
                d = d.max(x);
            }
            Some(d)
        };
        let mut iter = CofacetIter::new(bt, simplex.index, dim, self.len());
        if upper_only {
            while let Some((index, vertex)) = iter.next_upper() {
                let Some(diameter) = cofacet_diameter(vertex) else {
                    continue;
                };
                let cofacet = Cofacet {
                    index,
                    k: 0,
                    vertex,
                    diameter,
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        } else {
            while let Some((index, vertex, k)) = iter.next_all() {
                let Some(diameter) = cofacet_diameter(vertex) else {
                    continue;
                };
                let cofacet = Cofacet {
                    index,
                    k,
                    vertex,
                    diameter,
                };
                if let ControlFlow::Break(t) = f(cofacet) {
                    return Some(t);
                }
            }
        }
        None
    }
}
