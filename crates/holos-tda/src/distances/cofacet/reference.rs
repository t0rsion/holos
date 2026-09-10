use std::ops::ControlFlow;

use crate::combinadic::BinomialTable;
use crate::simplex::Simplex;

use super::Cofacet;
use super::sparse::advance_cofacet_index;
use crate::distances::SparseDistanceMatrix;

/// The sparse cofacet algorithm from 0.5.0, retained as a test reference.
/// It builds the complete candidate set before it emits callbacks.
impl SparseDistanceMatrix {
    pub(crate) fn for_each_cofacet_reference<T>(
        &self,
        bt: &BinomialTable,
        simplex: Simplex,
        verts: &[usize],
        dim: usize,
        upper_only: bool,
        mut f: impl FnMut(Cofacet) -> ControlFlow<T>,
    ) -> Option<T> {
        let candidates = self.reference_candidates(simplex, verts);
        emit_reference_candidates(bt, simplex, verts, dim, upper_only, &candidates, &mut f)
    }

    fn reference_candidates(&self, simplex: Simplex, verts: &[usize]) -> Vec<(usize, f64)> {
        let pivot = *verts
            .iter()
            .min_by_key(|&&v| self.degree(v))
            .expect("cofacet enumeration needs a non-empty simplex");
        let (start, end) = self.span(pivot);
        let mut candidates: Vec<(usize, f64)> = Vec::new();
        'w: for &w in &self.indices[start..end] {
            let w = w as usize;
            if verts.binary_search(&w).is_ok() {
                continue;
            }
            let mut diameter = simplex.diameter;
            for &v in verts {
                let d = self.get(w, v);
                if !d.is_finite() {
                    continue 'w;
                }
                diameter = diameter.max(d);
            }
            candidates.push((w, diameter));
        }
        candidates
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_reference_candidates<T>(
    table: &BinomialTable,
    simplex: Simplex,
    vertices: &[usize],
    dimension: usize,
    upper_only: bool,
    candidates: &[(usize, f64)],
    callback: &mut impl FnMut(Cofacet) -> ControlFlow<T>,
) -> Option<T> {
    let mut below = simplex.index;
    let mut above = 0u64;
    let mut position = dimension + 1;
    for &(vertex, diameter) in candidates.iter().rev() {
        advance_cofacet_index(
            vertices,
            vertex,
            &mut position,
            &mut below,
            &mut above,
            table,
        );
        if upper_only && position != dimension + 1 {
            break;
        }
        let cofacet = Cofacet {
            index: above + table.get(vertex, position + 1) + below,
            k: if upper_only { 0 } else { position },
            vertex,
            diameter,
        };
        if let ControlFlow::Break(value) = callback(cofacet) {
            return Some(value);
        }
    }
    None
}
