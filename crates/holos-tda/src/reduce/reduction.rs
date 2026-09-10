use std::collections::BinaryHeap;

use crate::distances::Distances;
use crate::field::{Coeffs, Entry, HeapEntry};
use crate::simplex::Simplex;
use crate::{Bar, Diagram};

use super::model::SerialReduction;
use super::{Engine, Pivots, RawH1Class, RawH1Term};

impl<'a, C: Coeffs + Sync, D: Distances + Sync> Engine<'a, C, D> {
    /// Reduce the dim-d columns serially against implicit (d+1)-rows.
    pub(crate) fn reduce_dimension(
        &self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        diagram: &mut Diagram,
    ) -> Pivots {
        self.reduce_dimension_impl(columns, dim, prev_pivots, diagram, None)
    }

    pub(super) fn reduce_dimension_impl(
        &self,
        columns: &[Simplex],
        dim: usize,
        prev_pivots: &Pivots,
        diagram: &mut Diagram,
        classes: Option<&mut Vec<RawH1Class>>,
    ) -> Pivots {
        let mut state = SerialReduction::new(columns.len(), classes);
        for (col_pos, &column) in columns.iter().enumerate() {
            self.reduce_serial_column(
                columns,
                column,
                col_pos,
                dim,
                prev_pivots,
                diagram,
                &mut state,
            );
        }
        state.pivots
    }

    #[allow(clippy::too_many_arguments)]
    fn reduce_serial_column(
        &self,
        columns: &[Simplex],
        column: Simplex,
        col_pos: usize,
        dim: usize,
        prev_pivots: &Pivots,
        diagram: &mut Diagram,
        state: &mut SerialReduction<'_>,
    ) {
        state.coboundary.clear();
        state.reduction.clear();
        let mut pivot = self.init_coboundary(
            column,
            dim,
            |index| state.pivots.contains_key(&index),
            &mut state.coboundary,
            &mut state.cofacets,
            &mut state.vertices,
            &mut state.cofacet_vertices,
            &mut state.pairs,
        );
        if self.record_emergent(column, pivot, col_pos, dim, diagram, state) {
            return;
        }
        while let Some(entry) = pivot {
            let index = self.ops.index(entry);
            if let Some(&(coefficient, position)) = state.pivots.get(&index) {
                let span = state.offsets[position]..state.offsets[position + 1];
                self.fold_reducer(
                    entry,
                    coefficient,
                    columns[position],
                    &state.entries[span],
                    dim,
                    &mut state.reduction,
                    &mut state.coboundary,
                    &mut state.vertices,
                );
                pivot = self.get_pivot(&mut state.coboundary);
                continue;
            }
            if let Some(apparent) = self.reduce_apparent_facet(
                entry,
                dim,
                &mut state.reduction,
                &mut state.coboundary,
                &mut state.vertices,
                &mut state.pairs,
            ) {
                pivot = apparent;
                continue;
            }
            self.record_paired_column(column, entry, col_pos, dim, diagram, state);
            state.offsets.push(state.entries.len());
            return;
        }
        self.record_zero_column(column, dim, prev_pivots, diagram, state);
        state.offsets.push(state.entries.len());
    }

    #[allow(clippy::too_many_arguments)]
    fn record_emergent(
        &self,
        column: Simplex,
        pivot: Option<Entry>,
        col_pos: usize,
        dim: usize,
        diagram: &mut Diagram,
        state: &mut SerialReduction<'_>,
    ) -> bool {
        let Some(entry) = pivot else {
            return false;
        };
        if !state.coboundary.is_empty() {
            return false;
        }
        debug_assert!(state.reduction.is_empty());
        self.emit_pair(column, self.ops.simplex(entry), dim, diagram);
        if let Some(classes) = state.classes.as_deref_mut() {
            self.record_h1_class(column, Some(self.ops.simplex(entry)), &[], classes);
        }
        state
            .pivots
            .insert(self.ops.index(entry), (self.ops.coeff(entry), col_pos));
        state.offsets.push(state.entries.len());
        true
    }

    #[allow(clippy::too_many_arguments)]
    fn record_paired_column(
        &self,
        column: Simplex,
        pivot: Entry,
        col_pos: usize,
        dim: usize,
        diagram: &mut Diagram,
        state: &mut SerialReduction<'_>,
    ) {
        self.emit_pair(column, self.ops.simplex(pivot), dim, diagram);
        state
            .pivots
            .insert(self.ops.index(pivot), (self.ops.coeff(pivot), col_pos));
        let start = state.entries.len();
        self.drain_into(&mut state.reduction, &mut state.entries);
        if let Some(classes) = state.classes.as_deref_mut() {
            self.record_h1_class(
                column,
                Some(self.ops.simplex(pivot)),
                &state.entries[start..],
                classes,
            );
        }
    }

    fn record_zero_column(
        &self,
        column: Simplex,
        dim: usize,
        prev_pivots: &Pivots,
        diagram: &mut Diagram,
        state: &mut SerialReduction<'_>,
    ) {
        let is_prior_death = !self.params.use_clearing && prev_pivots.contains_key(&column.index);
        if is_prior_death {
            return;
        }
        diagram.bars.push(Bar {
            dim,
            birth: column.diameter,
            death: f64::INFINITY,
        });
        if let Some(classes) = state.classes.as_deref_mut() {
            state.essential_terms.clear();
            self.drain_into(&mut state.reduction, &mut state.essential_terms);
            self.record_h1_class(column, None, &state.essential_terms, classes);
        }
    }

    fn record_h1_class(
        &self,
        leading: Simplex,
        death: Option<Simplex>,
        reduction: &[Entry],
        classes: &mut Vec<RawH1Class>,
    ) {
        let (death_value, scale) = match death {
            Some(death) if death.diameter > leading.diameter => {
                (death.diameter, previous_float(death.diameter))
            }
            Some(_) => return,
            None => (
                f64::INFINITY,
                self.effective_threshold.min(self.dist.max_distance()),
            ),
        };
        let mut terms = Vec::with_capacity(reduction.len() + 1);
        terms.push(RawH1Term {
            simplex: leading,
            coefficient: 1,
        });
        terms.extend(reduction.iter().map(|&entry| RawH1Term {
            simplex: self.ops.simplex(entry),
            coefficient: self.ops.coeff(entry),
        }));
        classes.push(RawH1Class {
            bar: Bar {
                dim: 1,
                birth: leading.diameter,
                death: death_value,
            },
            scale,
            birth: leading,
            death,
            terms,
        });
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
}

/// Largest finite floating-point value below a positive value. Persistent
/// deaths are positive because zero-persistence pairs are omitted.
fn previous_float(value: f64) -> f64 {
    debug_assert!(value > 0.0 && !value.is_nan());
    f64::from_bits(value.to_bits() - 1)
}
