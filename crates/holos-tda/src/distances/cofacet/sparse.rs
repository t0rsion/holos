use std::ops::ControlFlow;

use crate::combinadic::BinomialTable;
use crate::simplex::Simplex;

use super::{Cofacet, Distances, INLINE_VERTS, counters};
use crate::distances::SparseDistanceMatrix;

impl Distances for SparseDistanceMatrix {
    fn len(&self) -> usize {
        self.n
    }

    fn get(&self, i: usize, j: usize) -> f64 {
        SparseDistanceMatrix::get(self, i, j)
    }

    /// Sparse input has no enclosing radius: absent edges are absent at
    /// every scale. The default therefore includes all listed edges.
    fn default_threshold(&self) -> f64 {
        f64::INFINITY
    }

    fn max_distance(&self) -> f64 {
        self.max_distance
    }

    fn for_each_edge(&self, mut f: impl FnMut(usize, usize, f64)) {
        for i in 0..self.n {
            let (start, end) = self.span(i);
            for (&j, &d) in self.indices[start..end]
                .iter()
                .zip(&self.values[start..end])
            {
                let j = j as usize;
                if j < i {
                    f(i, j, d);
                }
            }
        }
    }

    /// Enumerate cofacets from the neighbor lists, as ripser's sparse
    /// coboundary does. An in-complex cofacet adds a vertex adjacent to
    /// every simplex vertex, so the enumeration merges the vertices'
    /// neighbor lists from their high ends instead of scanning all `n`
    /// candidates. It streams: each cofacet reaches `f` as soon as the
    /// merge finds it, so a `Break` stops the merge as well as the
    /// callbacks. A simplex vertex never appears in its own neighbor list,
    /// so the merge excludes the simplex vertices without a separate test.
    /// The descent reads `indices` and takes a distance from `values` only
    /// where every list holds the same vertex.
    ///
    /// The index, the position `k`, and the diameter match the dense
    /// default bit for bit. The index recurrence is the one
    /// [`crate::combinadic::CofacetIter::advance`] runs, and the diameter
    /// folds the same values in the same order. The enumeration omits
    /// cofacets whose diameter is infinite.
    ///
    /// Under `BOUNDED` the merge drops a candidate as soon as one of its
    /// distances exceeds `bound`. The cursors of the lists it did not reach
    /// stay where they stand. Every later candidate is smaller, so those
    /// cursors pass the same entries then and read nothing twice.
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
        assert!(
            !verts.is_empty(),
            "cofacet enumeration needs a non-empty simplex"
        );
        let width = verts.len();
        let indices = &self.indices[..];
        let values = &self.values[..];
        let mut inline = [(0usize, 0usize); INLINE_VERTS];
        let mut spill: Vec<(usize, usize)>;
        let cursor: &mut [(usize, usize)] = if width <= INLINE_VERTS {
            &mut inline[..width]
        } else {
            counters::note_spill();
            spill = vec![(0, 0); width];
            &mut spill
        };
        for (slot, &v) in cursor.iter_mut().zip(verts) {
            *slot = self.span(v);
        }
        let floor = cofacet_floor(cursor[0], indices, verts, upper_only);
        let mut idx_below = simplex.index;
        let mut idx_above = 0u64;
        let mut k = dim + 1;
        while let Some((w, first_distance)) =
            next_driver_candidate::<BOUNDED>(cursor, floor, indices, values, bound)
        {
            let diameter = match match_sparse_candidate::<BOUNDED>(
                &mut cursor[1..],
                w,
                indices,
                values,
                bound,
                simplex.diameter.max(first_distance),
            ) {
                CandidateMatch::Exhausted => return None,
                CandidateMatch::Rejected => continue,
                CandidateMatch::Matched(diameter) => diameter,
            };
            let w = w as usize;
            advance_cofacet_index(verts, w, &mut k, &mut idx_below, &mut idx_above, bt);
            debug_assert!(!upper_only || k == dim + 1);
            let cofacet = Cofacet {
                index: idx_above + bt.get(w, k + 1) + idx_below,
                k: if upper_only { 0 } else { k },
                vertex: w,
                diameter,
            };
            counters::note_callback();
            if let ControlFlow::Break(t) = f(cofacet) {
                counters::note_break();
                return Some(t);
            }
        }
        None
    }
}

fn cofacet_floor(
    driver: (usize, usize),
    indices: &[u32],
    vertices: &[usize],
    upper_only: bool,
) -> usize {
    if !upper_only {
        return driver.0;
    }
    let (start, end) = driver;
    let highest = vertices[vertices.len() - 1];
    start + indices[start..end].partition_point(|&vertex| vertex as usize <= highest)
}

fn next_driver_candidate<const BOUNDED: bool>(
    cursor: &mut [(usize, usize)],
    floor: usize,
    indices: &[u32],
    values: &[f64],
    bound: f64,
) -> Option<(u32, f64)> {
    while cursor[0].1 != floor {
        cursor[0].1 -= 1;
        counters::note_candidate();
        let at = cursor[0].1;
        if !BOUNDED || values[at] <= bound {
            return Some((indices[at], values[at]));
        }
    }
    None
}

enum CandidateMatch {
    Exhausted,
    Rejected,
    Matched(f64),
}

fn match_sparse_candidate<const BOUNDED: bool>(
    cursors: &mut [(usize, usize)],
    candidate: u32,
    indices: &[u32],
    values: &[f64],
    bound: f64,
    mut diameter: f64,
) -> CandidateMatch {
    for cursor in cursors {
        loop {
            if cursor.1 == cursor.0 {
                return CandidateMatch::Exhausted;
            }
            counters::note_candidate();
            let at = cursor.1 - 1;
            match indices[at].cmp(&candidate) {
                std::cmp::Ordering::Greater => cursor.1 = at,
                std::cmp::Ordering::Less => return CandidateMatch::Rejected,
                std::cmp::Ordering::Equal => {
                    cursor.1 = at;
                    if BOUNDED && values[at] > bound {
                        return CandidateMatch::Rejected;
                    }
                    diameter = diameter.max(values[at]);
                    break;
                }
            }
        }
    }
    CandidateMatch::Matched(diameter)
}

pub(crate) fn advance_cofacet_index(
    vertices: &[usize],
    candidate: usize,
    position: &mut usize,
    below: &mut u64,
    above: &mut u64,
    table: &BinomialTable,
) {
    while *position >= 1 && vertices[*position - 1] > candidate {
        *below -= table.get(vertices[*position - 1], *position);
        *above += table.get(vertices[*position - 1], *position + 1);
        *position -= 1;
    }
}
