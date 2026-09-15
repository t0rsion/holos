//! Certificate construction and reduction repair.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashMap;

use crate::{
    Cocycle, CriticalPair, Diagram, RipsParams, SparseDistanceMatrix, rips_persistence_sparse,
};

use super::cycles::{CheckedWitnessParts, CycleWitness};
use super::model::{
    CertificateError, CertificateLimits, CertificateResult, CertifiedReductionRegion, ChangeColumn,
    FiltrationSimplex, ReductionCertificate, ReductionRepair, ReductionRepairMode,
    ReductionRepairWork, RegionH1Pair,
};
use super::reduction::{
    DimensionRepair, FilteredComplex, SparseColumn, reduce_with_basis, reduce_with_prefix,
    reindex_change_columns, reindexed_prefix_candidates, valid_reduction_prefix_len,
};
use super::region::{region_value_formula, simplex_rank};
use super::verify::{
    CheckedReductions, add_change_guards, add_pivot_guards, check_reductions, checked_threshold,
    diagram_bits_equal, graph_digest, minimize_guards, validate_header,
};

fn build_checked_reductions(
    input: &SparseDistanceMatrix,
    modulus: u32,
    threshold: f64,
    limits: CertificateLimits,
) -> CertificateResult<(
    FilteredComplex,
    Vec<ChangeColumn>,
    Vec<ChangeColumn>,
    CheckedReductions,
)> {
    let complex = FilteredComplex::build(input, threshold, limits)?;
    let edge_columns = reduce_with_basis(&complex.edge_boundaries(modulus), modulus, limits)?;
    let triangle_columns =
        reduce_with_basis(&complex.triangle_boundaries(modulus), modulus, limits)?;
    let checked = check_reductions(&complex, modulus, &edge_columns, &triangle_columns, limits)?;
    Ok((complex, edge_columns, triangle_columns, checked))
}

fn check_compute_diagram(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    checked: &Diagram,
) -> CertificateResult<()> {
    let computed = rips_persistence_sparse(input, params)
        .map_err(|error| CertificateError::new(error.to_string()))?;
    if !diagram_bits_equal(&computed, checked) {
        return Err(CertificateError::new(format!(
            "reference certificate diagram differs from the compute engine: expected {:?}, got {:?}",
            computed.bars, checked.bars
        )));
    }
    Ok(())
}
fn complex_edges(complex: &FilteredComplex) -> Vec<[usize; 2]> {
    complex
        .edges
        .iter()
        .map(|simplex| simplex.vertices)
        .collect()
}

fn complex_triangles(complex: &FilteredComplex) -> Vec<[usize; 3]> {
    complex
        .triangles
        .iter()
        .map(|simplex| simplex.vertices)
        .collect()
}

fn check_update_topology(
    current: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    operation: &str,
) -> CertificateResult<()> {
    if current.len() != updated.len() {
        return Err(CertificateError::new(format!(
            "reduction {operation} requires an unchanged vertex set"
        )));
    }
    let current_topology: Vec<_> = current.edges().map(|(u, v, _)| [u, v]).collect();
    let updated_topology: Vec<_> = updated.edges().map(|(u, v, _)| [u, v]).collect();
    if current_topology != updated_topology {
        return Err(CertificateError::new(format!(
            "reduction {operation} requires an unchanged listed edge set"
        )));
    }
    Ok(())
}

fn check_update_contract(
    current: &SparseDistanceMatrix,
    updated: &SparseDistanceMatrix,
    threshold: f64,
    operation: &str,
) -> CertificateResult<()> {
    check_update_topology(current, updated, operation)?;
    let current_active: Vec<_> = current
        .edges()
        .map(|(_, _, value)| value <= threshold)
        .collect();
    let updated_active: Vec<_> = updated
        .edges()
        .map(|(_, _, value)| value <= threshold)
        .collect();
    if current_active != updated_active {
        return Err(CertificateError::new(format!(
            "reduction {operation} requires unchanged threshold membership"
        )));
    }
    Ok(())
}

fn repair_dimension<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ChangeColumn],
    boundaries: &[SparseColumn],
    modulus: u32,
    limits: CertificateLimits,
) -> CertificateResult<DimensionRepair> {
    let candidates = reindexed_prefix_candidates(old_simplices, new_simplices, old_columns)?;
    let prefix = valid_reduction_prefix_len(boundaries, &candidates, modulus, limits)?;
    let (columns, additions) =
        reduce_with_prefix(boundaries, &candidates[..prefix], modulus, limits)?;
    Ok(DimensionRepair {
        columns,
        prefix,
        additions,
    })
}

fn repair_mode(work: ReductionRepairWork) -> ReductionRepairMode {
    if work.columns_reduced() == 0 {
        ReductionRepairMode::Reused
    } else if work.columns_reused() == 0 {
        ReductionRepairMode::Rebuilt
    } else {
        ReductionRepairMode::SuffixRepaired
    }
}
impl ReductionCertificate {
    fn build_parts(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
    ) -> CertificateResult<(Self, FilteredComplex, CheckedReductions)> {
        validate_header(input, params.max_dim, params.modulus, limits)?;
        let threshold = checked_threshold(params.threshold)?;
        let (complex, edge_columns, triangle_columns, checked) =
            build_checked_reductions(input, params.modulus, threshold, limits)?;
        check_compute_diagram(input, params, &checked.diagram)?;
        let certificate = Self {
            vertex_count: input.len(),
            threshold: params.threshold,
            modulus: params.modulus,
            graph_digest: graph_digest(input, threshold),
            edge_columns,
            triangle_columns,
            diagram: checked.diagram.clone(),
        };
        Ok((certificate, complex, checked))
    }

    /// Produce an exact H0 and H1 reduction certificate.
    ///
    /// The producer uses an explicit reference reduction. It is separate
    /// from the implicit compute engine and is limited by
    /// [`CertificateLimits`].
    pub fn build(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
    ) -> std::result::Result<Self, CertificateError> {
        let (certificate, _, _) = Self::build_parts(input, params, limits)?;
        Ok(certificate)
    }

    pub(crate) fn build_cycle_witness(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        critical_pairs: &[CriticalPair],
        selected: &Cocycle,
        limits: CertificateLimits,
    ) -> CertificateResult<CycleWitness> {
        let (certificate, complex, checked) = Self::build_parts(input, params, limits)?;
        let parts = CheckedWitnessParts::new(&certificate, &complex, &checked);
        super::cycles::cycle_witness_from_checked(
            parts,
            input,
            params.threshold.unwrap_or(f64::INFINITY),
            params.modulus,
            critical_pairs,
            selected,
            limits,
        )
    }

    /// Adapt this checked reduction to new weights on the same listed graph.
    ///
    /// The repair reindexes dependencies by simplex identity in each boundary
    /// dimension. It retains the longest unit-triangular prefix whose
    /// recomputed columns still have distinct pivots, then reduces the suffix.
    ///
    /// The current graph must match this certificate. The updated graph must
    /// keep the vertex set, listed edges, and threshold membership fixed.
    pub fn repair(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<ReductionRepair, CertificateError> {
        let (old_complex, _) = self.verify_parts(current, limits)?;
        let threshold = checked_threshold(self.threshold)?;
        check_update_contract(current, updated, threshold, "repair")?;
        let new_complex = FilteredComplex::build(updated, threshold, limits)?;
        let edge_repair = repair_dimension(
            &complex_edges(&old_complex),
            &complex_edges(&new_complex),
            &self.edge_columns,
            &new_complex.edge_boundaries(self.modulus),
            self.modulus,
            limits,
        )?;
        let triangle_repair = repair_dimension(
            &complex_triangles(&old_complex),
            &complex_triangles(&new_complex),
            &self.triangle_columns,
            &new_complex.triangle_boundaries(self.modulus),
            self.modulus,
            limits,
        )?;
        let checked = check_reductions(
            &new_complex,
            self.modulus,
            &edge_repair.columns,
            &triangle_repair.columns,
            limits,
        )?;
        let certificate = Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            graph_digest: graph_digest(updated, threshold),
            edge_columns: edge_repair.columns,
            triangle_columns: triangle_repair.columns,
            diagram: checked.diagram,
        };
        let work = ReductionRepairWork {
            edge_columns_reused: edge_repair.prefix,
            edge_columns_reduced: new_complex.edges.len() - edge_repair.prefix,
            triangle_columns_reused: triangle_repair.prefix,
            triangle_columns_reduced: new_complex.triangles.len() - triangle_repair.prefix,
            column_additions: edge_repair.additions + triangle_repair.additions,
        };
        Ok(ReductionRepair {
            certificate,
            mode: repair_mode(work),
            work,
        })
    }

    /// Reindex this reduction after a result-sensitive accepted update.
    ///
    /// No boundary column is reduced. Simplex identities remap every source
    /// and target position in `V`.
    pub fn reindex(
        &self,
        current: &SparseDistanceMatrix,
        updated: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<Self, CertificateError> {
        let (old_complex, _) = self.verify_parts(current, limits)?;
        let threshold = checked_threshold(self.threshold)?;
        check_update_topology(current, updated, "reindexing")?;
        let new_complex = FilteredComplex::build(updated, threshold, limits)?;
        if old_complex.edges.len() != new_complex.edges.len()
            || old_complex.triangles.len() != new_complex.triangles.len()
        {
            return Err(CertificateError::new(
                "reduction reindexing requires unchanged threshold membership",
            ));
        }
        let edge_columns = reindex_change_columns(
            &complex_edges(&old_complex),
            &complex_edges(&new_complex),
            &self.edge_columns,
        )?;
        let triangle_columns = reindex_change_columns(
            &complex_triangles(&old_complex),
            &complex_triangles(&new_complex),
            &self.triangle_columns,
        )?;
        let checked = check_reductions(
            &new_complex,
            self.modulus,
            &edge_columns,
            &triangle_columns,
            limits,
        )?;
        Ok(Self {
            vertex_count: updated.len(),
            threshold: self.threshold,
            modulus: self.modulus,
            graph_digest: graph_digest(updated, threshold),
            edge_columns,
            triangle_columns,
            diagram: checked.diagram,
        })
    }

    /// Vertex count bound to this certificate.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Filtration threshold bound to this certificate.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Digest of the thresholded graph.
    pub fn graph_digest(&self) -> &[u8; 32] {
        &self.graph_digest
    }

    /// Edge-boundary change-of-basis columns.
    pub fn edge_columns(&self) -> &[ChangeColumn] {
        &self.edge_columns
    }

    /// Triangle-boundary change-of-basis columns.
    pub fn triangle_columns(&self) -> &[ChangeColumn] {
        &self.triangle_columns
    }

    /// Diagram derived from the checked reduction.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }
}

impl ReductionCertificate {
    /// Compile the checked factorization into a result-sensitive region.
    ///
    /// The region stays valid under edge-order changes that do not break
    /// a filtration dependency in `V` or a reduced pivot in `R`.
    pub fn compile_region(
        &self,
        input: &SparseDistanceMatrix,
        limits: CertificateLimits,
    ) -> std::result::Result<CertifiedReductionRegion, CertificateError> {
        let (complex, checked) = self.verify_parts(input, limits)?;
        let edge_simplices: Vec<_> = complex
            .edges
            .iter()
            .map(|edge| FiltrationSimplex::new(edge.vertices.to_vec()))
            .collect();
        let triangle_simplices: Vec<_> = complex
            .triangles
            .iter()
            .map(|triangle| FiltrationSimplex::new(triangle.vertices.to_vec()))
            .collect();
        let mut guards = BTreeSet::new();
        add_change_guards(&mut guards, &self.edge_columns, &edge_simplices);
        add_change_guards(&mut guards, &self.triangle_columns, &triangle_simplices);
        add_pivot_guards(&mut guards, &checked.reduced_triangles, &edge_simplices);

        let mut killed_vertices = vec![false; complex.vertex_count];
        let mut h0_deaths = Vec::new();
        for (column, reduced) in checked.reduced_edges.iter().enumerate() {
            if let Some((pivot, _)) = reduced.pivot() {
                killed_vertices[pivot] = true;
                h0_deaths.push(complex.edges[column].vertices);
            }
        }
        let h0_essential = killed_vertices.iter().filter(|&&killed| !killed).count();
        let mut h1_deaths = FxHashMap::default();
        for (column, reduced) in checked.reduced_triangles.iter().enumerate() {
            if let Some((pivot, _)) = reduced.pivot() {
                h1_deaths.insert(pivot, complex.triangles[column].vertices);
            }
        }
        let h1_pairs: Vec<_> = checked
            .reduced_edges
            .iter()
            .enumerate()
            .filter(|(_, reduced)| reduced.0.is_empty())
            .map(|(edge, _)| RegionH1Pair {
                birth: complex.edges[edge].vertices,
                death: h1_deaths.get(&edge).copied(),
            })
            .collect();
        let threshold = checked_threshold(self.threshold)?;
        let topology: Vec<_> = input.edges().map(|(u, v, _)| [u, v]).collect();
        let active = input
            .edges()
            .map(|(_, _, value)| value <= threshold)
            .collect();
        let complete_guards: Vec<_> = guards.iter().cloned().collect();
        let guards = minimize_guards(guards);
        let mut simplex_indices = BTreeMap::new();
        for guard in &guards {
            for simplex in [&guard.earlier, &guard.later] {
                let next = simplex_indices.len();
                simplex_indices.entry(simplex.clone()).or_insert(next);
            }
        }
        let mut guard_simplices = vec![FiltrationSimplex::new(Vec::new()); simplex_indices.len()];
        for (simplex, index) in &simplex_indices {
            guard_simplices[*index] = simplex.clone();
        }
        let guard_indices = guards
            .iter()
            .map(|guard| {
                (
                    simplex_indices[&guard.earlier],
                    simplex_indices[&guard.later],
                )
            })
            .collect();
        let guard_ranks = guard_simplices.iter().map(simplex_rank).collect();
        let edge_indices: BTreeMap<_, _> = topology
            .iter()
            .copied()
            .enumerate()
            .map(|(index, edge)| (edge, index))
            .collect();
        let guard_formulas = guard_simplices
            .iter()
            .map(|simplex| region_value_formula(simplex, &edge_indices))
            .collect();
        let h1_formulas = h1_pairs
            .iter()
            .map(|pair| {
                (
                    edge_indices[&pair.birth],
                    pair.death.map(|[u, v, w]| {
                        [
                            edge_indices[&[u, v]],
                            edge_indices[&[u, w]],
                            edge_indices[&[v, w]],
                        ]
                    }),
                )
            })
            .collect();
        Ok(CertifiedReductionRegion {
            vertex_count: input.len(),
            threshold: self.threshold,
            topology,
            active,
            complete_guards,
            guards,
            guard_indices,
            guard_ranks,
            guard_formulas,
            h0_deaths,
            h0_essential,
            h1_pairs,
            h1_formulas,
        })
    }
}
