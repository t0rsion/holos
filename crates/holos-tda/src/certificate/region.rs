//! Result-sensitive reduction regions.

use std::collections::BTreeMap;

use crate::{Bar, CriticalPair, CriticalSimplex, Diagram, SparseDistanceMatrix};

use super::model::{
    CertificateError, CertifiedReductionRegion, CertifiedRegionEvaluation, FiltrationSimplex,
    ReductionGuard, RegionValueFormula, RegionViolation, RegionViolationKind,
};
use super::verify::{edge_rank, triangle_rank};

impl CertifiedReductionRegion {
    /// Labeled vertex count fixed by the region.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// All distinct comparisons derived before transitive removal.
    pub fn complete_guards(&self) -> &[ReductionGuard] {
        &self.complete_guards
    }

    /// Transitively reduced comparisons checked during evaluation.
    pub fn guards(&self) -> &[ReductionGuard] {
        &self.guards
    }

    /// Return every failed topology or algebraic condition.
    pub fn violations(&self, updated: &SparseDistanceMatrix) -> Vec<RegionViolation> {
        let mut violations = Vec::new();
        if updated.len() != self.vertex_count {
            violations.push(RegionViolation {
                kind: RegionViolationKind::VertexSetChanged,
                guard_index: None,
                first: None,
                second: None,
            });
            return violations;
        }
        let topology: Vec<_> = updated.edges().map(|(u, v, _)| [u, v]).collect();
        if topology != self.topology {
            let first = self
                .topology
                .iter()
                .find(|edge| topology.binary_search(edge).is_err())
                .or_else(|| {
                    topology
                        .iter()
                        .find(|edge| self.topology.binary_search(edge).is_err())
                })
                .copied()
                .map(FiltrationSimplex::new);
            violations.push(RegionViolation {
                kind: RegionViolationKind::EdgeSetChanged,
                guard_index: None,
                first,
                second: None,
            });
            return violations;
        }
        let threshold = self.threshold.unwrap_or(f64::INFINITY);
        for (index, [u, v]) in self.topology.iter().copied().enumerate() {
            if (updated.get(u, v) <= threshold) != self.active[index] {
                violations.push(RegionViolation {
                    kind: RegionViolationKind::ThresholdCrossing,
                    guard_index: None,
                    first: Some(FiltrationSimplex::new(vec![u, v])),
                    second: None,
                });
            }
        }
        if !violations.is_empty() {
            return violations;
        }
        let edge_values: Vec<_> = updated.edges().map(|(_, _, value)| value).collect();
        let values: Vec<_> = self
            .guard_formulas
            .iter()
            .map(|formula| formula.value(|edge| edge_values[edge]))
            .collect();
        for (index, (&(earlier, later), guard)) in
            self.guard_indices.iter().zip(&self.guards).enumerate()
        {
            let order = values[earlier]
                .total_cmp(&values[later])
                .then_with(|| self.guard_ranks[later].cmp(&self.guard_ranks[earlier]));
            if order.is_gt() {
                violations.push(RegionViolation {
                    kind: RegionViolationKind::GuardFailed,
                    guard_index: Some(index),
                    first: Some(guard.earlier.clone()),
                    second: Some(guard.later.clone()),
                });
            }
        }
        violations
    }

    /// Evaluate the exact diagram without reduction when every guard holds.
    pub fn evaluate(
        &self,
        updated: &SparseDistanceMatrix,
    ) -> std::result::Result<CertifiedRegionEvaluation, CertificateError> {
        let violations = self.violations(updated);
        if let Some(first) = violations.first() {
            return Err(CertificateError::new(format!(
                "certified region ended at {:?}",
                first.kind
            )));
        }
        let mut diagram = Diagram::default();
        for &[u, v] in &self.h0_deaths {
            let death = updated.get(u, v);
            if death > 0.0 {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
        for _ in 0..self.h0_essential {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death: f64::INFINITY,
            });
        }
        let mut h1_pairs = Vec::new();
        for pair in &self.h1_pairs {
            let birth = updated.get(pair.birth[0], pair.birth[1]);
            let death = pair
                .death
                .map(|vertices| triangle_value(updated, vertices))
                .unwrap_or(f64::INFINITY);
            if death > birth {
                let interval = Bar {
                    dim: 1,
                    birth,
                    death,
                };
                diagram.bars.push(interval);
                h1_pairs.push((
                    interval,
                    CriticalPair {
                        birth: CriticalSimplex {
                            vertices: pair.birth.to_vec(),
                            value: birth,
                        },
                        death: pair.death.map(|vertices| CriticalSimplex {
                            vertices: vertices.to_vec(),
                            value: triangle_value(updated, vertices),
                        }),
                    },
                ));
            }
        }
        diagram.canonicalize();
        h1_pairs.sort_by(critical_pair_record_order);
        Ok(CertifiedRegionEvaluation {
            diagram,
            h1_pairs,
            guards_checked: self.guards.len(),
        })
    }

    pub(crate) fn evaluate_h1_indexed(
        &self,
        edge_values: &[f64],
        edge_positions: &[usize],
    ) -> std::result::Result<Vec<Bar>, CertificateError> {
        debug_assert_eq!(edge_positions.len(), self.topology.len());
        let edge_value = |edge: usize| edge_values[edge_positions[edge]];
        let values: Vec<_> = self
            .guard_formulas
            .iter()
            .map(|formula| formula.value(edge_value))
            .collect();
        for &(earlier, later) in &self.guard_indices {
            let order = values[earlier]
                .total_cmp(&values[later])
                .then_with(|| self.guard_ranks[later].cmp(&self.guard_ranks[earlier]));
            if order.is_gt() {
                return Err(CertificateError::new(
                    "certified region ended at GuardFailed",
                ));
            }
        }
        let mut bars = Vec::new();
        for &(birth_edge, death_edges) in &self.h1_formulas {
            let birth = edge_value(birth_edge);
            let death = death_edges
                .map(|[first, second, third]| {
                    edge_value(first)
                        .max(edge_value(second))
                        .max(edge_value(third))
                })
                .unwrap_or(f64::INFINITY);
            if death > birth {
                bars.push(Bar {
                    dim: 1,
                    birth,
                    death,
                });
            }
        }
        bars.sort_by(|left, right| {
            left.birth
                .total_cmp(&right.birth)
                .then(left.death.total_cmp(&right.death))
        });
        Ok(bars)
    }
}
pub(super) fn region_value_formula(
    simplex: &FiltrationSimplex,
    edge_indices: &BTreeMap<[usize; 2], usize>,
) -> RegionValueFormula {
    match *simplex.vertices.as_slice() {
        [_] => RegionValueFormula::Vertex,
        [u, v] => RegionValueFormula::Edge(edge_indices[&[u, v]]),
        [u, v, w] => RegionValueFormula::Triangle([
            edge_indices[&[u, v]],
            edge_indices[&[u, w]],
            edge_indices[&[v, w]],
        ]),
        _ => unreachable!("certified regions contain vertices, edges, and triangles"),
    }
}

pub(super) fn simplex_rank(simplex: &FiltrationSimplex) -> u128 {
    match *simplex.vertices.as_slice() {
        [u] => u as u128,
        [u, v] => edge_rank([u, v]),
        [u, v, w] => triangle_rank([u, v, w]),
        _ => unreachable!("certified regions contain vertices, edges, and triangles"),
    }
}

pub(super) fn triangle_value(matrix: &SparseDistanceMatrix, [u, v, w]: [usize; 3]) -> f64 {
    matrix.get(u, v).max(matrix.get(u, w)).max(matrix.get(v, w))
}

pub(super) fn critical_pair_record_order(
    a: &(Bar, CriticalPair),
    b: &(Bar, CriticalPair),
) -> std::cmp::Ordering {
    a.0.birth
        .total_cmp(&b.0.birth)
        .then(a.0.death.total_cmp(&b.0.death))
        .then(a.1.birth.vertices.cmp(&b.1.birth.vertices))
        .then_with(|| {
            a.1.death
                .as_ref()
                .map(|simplex| &simplex.vertices)
                .cmp(&b.1.death.as_ref().map(|simplex| &simplex.vertices))
        })
}
