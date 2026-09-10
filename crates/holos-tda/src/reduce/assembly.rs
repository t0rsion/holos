use std::ops::ControlFlow;

use rayon::prelude::*;

use crate::budget::Region;
use crate::distances::Distances;
use crate::field::Coeffs;
use crate::simplex::Simplex;

use super::pairing::insert_vertex;
use super::{Engine, PairScratch, Pivots};

impl<'a, C: Coeffs + Sync, D: Distances + Sync> Engine<'a, C, D> {
    /// Canonical cofacet assembly with clearing and apparent-pair pruning.
    /// Returns (all (d)-simplices, columns to reduce in dimension d). The
    /// simplex list seeds the next dimension's assembly. The final dimension
    /// has no such consumer, so `seed_next` is false there.
    ///
    /// Generation is per-simplex independent, so it runs in parallel over
    /// chunks. Concatenating in chunk order reproduces the serial output
    /// exactly, and the sort then fixes the column order.
    pub(super) fn assemble(
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
    pub(super) fn assemble_chunk(
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
                    // Only pay for the apparent-pair test when the cheaper
                    // clearing check has not already excluded the cofacet.
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
}
