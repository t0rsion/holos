use std::ops::ControlFlow;

use crate::combinadic::BinomialTable;
use crate::simplex::Simplex;

use super::{Cofacet, Distances};
use crate::distances::DistanceMatrix;

impl Distances for DistanceMatrix {
    fn len(&self) -> usize {
        self.n
    }

    fn get(&self, i: usize, j: usize) -> f64 {
        DistanceMatrix::get(self, i, j)
    }

    fn default_threshold(&self) -> f64 {
        self.enclosing_radius()
    }

    fn for_each_edge(&self, mut f: impl FnMut(usize, usize, f64)) {
        for i in 1..self.n {
            for j in 0..i {
                f(i, j, DistanceMatrix::get(self, i, j));
            }
        }
    }

    /// The dense walk. The diameter fold reads the storage form directly.
    ///
    /// Both forms take the vertices in ascending position, as the default
    /// does, so the diameters and the stopping point match bit for bit.
    #[allow(clippy::too_many_arguments)]
    fn enumerate_cofacets<const BOUNDED: bool, T, F>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        bound: f64,
        f: F,
    ) -> Option<T>
    where
        F: FnMut(Cofacet) -> ControlFlow<T>,
    {
        let data = &self.data;
        let n = self.n;
        let square = self.square;
        self.walk_cofacets(bt, simplex, dim, upper_only, f, |added| {
            let mut d = simplex.diameter;
            // The square form reads each simplex vertex's own row, which the
            // descending candidates walk backward, one contiguous run at a
            // time. The condensed form holds no row for the candidates above
            // a vertex, so it reads what the trait default reads. One walk
            // never mixes the two, so the test costs a predicted branch.
            let row = added * added.saturating_sub(1) / 2;
            for &v in verts {
                let x = if square {
                    data[v * n + added]
                } else if v < added {
                    data[row + v]
                } else {
                    data[v * (v - 1) / 2 + added]
                };
                if BOUNDED && x > bound {
                    return None;
                }
                d = d.max(x);
            }
            Some(d)
        })
    }
}

impl DistanceMatrix {
    /// The cofacet walk both storage forms share. `cofacet_diameter` folds
    /// one candidate's diameter and returns `None` for a candidate the
    /// caller's bound drops.
    #[inline]
    fn walk_cofacets<T, F, D>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        dim: usize,
        upper_only: bool,
        mut f: F,
        cofacet_diameter: D,
    ) -> Option<T>
    where
        F: FnMut(Cofacet) -> ControlFlow<T>,
        D: Fn(usize) -> Option<f64>,
    {
        let mut iter = crate::combinadic::CofacetIter::new(bt, simplex.index, dim, self.n);
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
