use std::collections::{BTreeMap, BTreeSet};

use crate::ProofError;
use crate::proof::{Graph, ProofColumn, ProofTerm, SparseColumn, check_matrix};

use super::claim::ReductionClaim;
use super::model::ProgramProofLimits;
use super::reduction::{Complex, checked_diagram, graph_digest};
use super::trace_reduction_replay::{replay_reduction_work, valid_prefix_len};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RepairWork {
    pub(super) edge_columns_reused: usize,
    pub(super) edge_columns_reduced: usize,
    pub(super) triangle_columns_reused: usize,
    pub(super) triangle_columns_reduced: usize,
    pub(super) column_additions: usize,
}

impl RepairWork {
    pub(super) fn columns_reused(self) -> Result<usize, ProofError> {
        self.edge_columns_reused
            .checked_add(self.triangle_columns_reused)
            .ok_or_else(|| ProofError::new("reused-column count overflows usize"))
    }

    pub(super) fn columns_reduced(self) -> Result<usize, ProofError> {
        self.edge_columns_reduced
            .checked_add(self.triangle_columns_reduced)
            .ok_or_else(|| ProofError::new("reduced-column count overflows usize"))
    }
}

pub(super) fn repair_work_for_graphs(
    old_graph: &Graph,
    new_graph: &Graph,
    claim: &ReductionClaim,
    limits: ProgramProofLimits,
) -> Result<RepairWork, ProofError> {
    if old_graph.vertex_count != new_graph.vertex_count
        || old_graph
            .edges
            .iter()
            .map(|edge| (edge.u, edge.v))
            .ne(new_graph.edges.iter().map(|edge| (edge.u, edge.v)))
    {
        return Err(ProofError::new(
            "reduction repair requires an unchanged listed graph",
        ));
    }
    let old_complex = Complex::build(
        old_graph,
        claim.threshold,
        limits.max_edges,
        limits.max_triangles,
    )?;
    let new_complex = Complex::build(
        new_graph,
        claim.threshold,
        limits.max_edges,
        limits.max_triangles,
    )?;
    let edge_boundaries = new_complex.edge_boundaries(claim.modulus);
    let triangle_boundaries = new_complex.triangle_boundaries(claim.modulus);
    let old_edge_simplices: Vec<_> = old_complex.edges.iter().map(|edge| edge.vertices).collect();
    let new_edge_simplices: Vec<_> = new_complex.edges.iter().map(|edge| edge.vertices).collect();
    let old_triangle_simplices: Vec<_> = old_complex
        .triangles
        .iter()
        .map(|triangle| triangle.vertices)
        .collect();
    let new_triangle_simplices: Vec<_> = new_complex
        .triangles
        .iter()
        .map(|triangle| triangle.vertices)
        .collect();
    let (edge_prefix, edge_additions) = repair_dimension_work(
        &old_edge_simplices,
        &new_edge_simplices,
        &claim.edge_columns,
        &edge_boundaries,
        claim.modulus,
        limits.max_terms,
    )?;
    let (triangle_prefix, triangle_additions) = repair_dimension_work(
        &old_triangle_simplices,
        &new_triangle_simplices,
        &claim.triangle_columns,
        &triangle_boundaries,
        claim.modulus,
        limits.max_terms,
    )?;
    Ok(RepairWork {
        edge_columns_reused: edge_prefix,
        edge_columns_reduced: new_complex.edges.len() - edge_prefix,
        triangle_columns_reused: triangle_prefix,
        triangle_columns_reduced: new_complex.triangles.len() - triangle_prefix,
        column_additions: edge_additions
            .checked_add(triangle_additions)
            .ok_or_else(|| ProofError::new("column addition count overflows usize"))?,
    })
}

pub(super) fn reindex_reduction_for_graphs(
    old_graph: &Graph,
    new_graph: &Graph,
    claim: &ReductionClaim,
    limits: ProgramProofLimits,
) -> Result<ReductionClaim, ProofError> {
    let old_complex = Complex::build(
        old_graph,
        claim.threshold,
        limits.max_edges,
        limits.max_triangles,
    )?;
    let new_complex = Complex::build(
        new_graph,
        claim.threshold,
        limits.max_edges,
        limits.max_triangles,
    )?;
    let edge_columns = reindex_columns(
        &old_complex
            .edges
            .iter()
            .map(|edge| edge.vertices)
            .collect::<Vec<_>>(),
        &new_complex
            .edges
            .iter()
            .map(|edge| edge.vertices)
            .collect::<Vec<_>>(),
        &claim.edge_columns,
    )?;
    let triangle_columns = reindex_columns(
        &old_complex
            .triangles
            .iter()
            .map(|triangle| triangle.vertices)
            .collect::<Vec<_>>(),
        &new_complex
            .triangles
            .iter()
            .map(|triangle| triangle.vertices)
            .collect::<Vec<_>>(),
        &claim.triangle_columns,
    )?;
    let reduced_edges = check_matrix(
        &new_complex.edge_boundaries(claim.modulus),
        &edge_columns,
        claim.modulus,
        "edge",
    )?;
    let reduced_triangles = check_matrix(
        &new_complex.triangle_boundaries(claim.modulus),
        &triangle_columns,
        claim.modulus,
        "triangle",
    )?;
    let (diagram, _) = checked_diagram(
        &new_complex,
        &reduced_edges,
        &reduced_triangles,
        limits.max_bars,
    )?;
    let mut result = claim.clone();
    result.edge_columns = edge_columns;
    result.triangle_columns = triangle_columns;
    result.diagram = diagram;
    result.graph_digest = graph_digest(new_graph, claim.threshold);
    Ok(result)
}

fn reindex_columns<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ProofColumn],
) -> Result<Vec<ProofColumn>, ProofError> {
    check_reindex_shapes(old_simplices, new_simplices, old_columns)?;
    let old_positions: BTreeMap<_, _> = old_simplices
        .iter()
        .copied()
        .enumerate()
        .map(|(index, simplex)| (simplex, index))
        .collect();
    let new_positions: BTreeMap<_, _> = new_simplices
        .iter()
        .copied()
        .enumerate()
        .map(|(index, simplex)| (simplex, index))
        .collect();
    check_reindex_sets(old_simplices, new_simplices, &old_positions, &new_positions)?;
    let mut columns = Vec::with_capacity(new_simplices.len());
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions[simplex];
        columns.push(reindex_column(
            new_target,
            &old_columns[old_target],
            old_simplices,
            &new_positions,
        )?);
    }
    Ok(columns)
}

fn check_reindex_shapes<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ProofColumn],
) -> Result<(), ProofError> {
    if old_simplices.len() != new_simplices.len() || old_columns.len() != old_simplices.len() {
        return Err(ProofError::new(
            "reindexing has inconsistent simplex counts",
        ));
    }
    Ok(())
}

fn check_reindex_sets<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_positions: &BTreeMap<[usize; N], usize>,
    new_positions: &BTreeMap<[usize; N], usize>,
) -> Result<(), ProofError> {
    if old_positions.len() != old_simplices.len()
        || new_positions.len() != new_simplices.len()
        || old_positions.keys().ne(new_positions.keys())
    {
        return Err(ProofError::new("reindexing found a changed simplex set"));
    }
    Ok(())
}

fn reindex_column<const N: usize>(
    new_target: usize,
    old_column: &ProofColumn,
    old_simplices: &[[usize; N]],
    new_positions: &BTreeMap<[usize; N], usize>,
) -> Result<ProofColumn, ProofError> {
    let mut terms = Vec::with_capacity(old_column.terms.len());
    for term in &old_column.terms {
        let source = old_simplices
            .get(term.index)
            .ok_or_else(|| ProofError::new("reindexing found an invalid source position"))?;
        let index = new_positions[source];
        if index > new_target {
            return Err(ProofError::new(
                "accepted reduction is not filtration-compatible after reindexing",
            ));
        }
        terms.push(ProofTerm {
            index,
            coefficient: term.coefficient,
        });
    }
    terms.sort_by_key(|term| term.index);
    if terms.windows(2).any(|pair| pair[0].index == pair[1].index) {
        return Err(ProofError::new(
            "reindexing produced duplicate source positions",
        ));
    }
    Ok(ProofColumn { terms })
}

fn repair_dimension_work<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ProofColumn],
    boundaries: &[SparseColumn],
    modulus: u32,
    term_limit: usize,
) -> Result<(usize, usize), ProofError> {
    check_repair_shapes(old_simplices, new_simplices, old_columns)?;
    let old_positions: BTreeMap<_, _> = old_simplices
        .iter()
        .copied()
        .enumerate()
        .map(|(index, simplex)| (simplex, index))
        .collect();
    let new_positions: BTreeMap<_, _> = new_simplices
        .iter()
        .copied()
        .enumerate()
        .map(|(index, simplex)| (simplex, index))
        .collect();
    let candidates = repair_candidates(
        old_simplices,
        new_simplices,
        old_columns,
        &old_positions,
        &new_positions,
    )?;
    let prefix = valid_prefix_len(&candidates, boundaries, modulus, term_limit)?;
    let additions = replay_reduction_work(&candidates[..prefix], boundaries, modulus, term_limit)?;
    Ok((prefix, additions))
}

fn check_repair_shapes<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ProofColumn],
) -> Result<(), ProofError> {
    let old_set: BTreeSet<_> = old_simplices.iter().copied().collect();
    let new_set: BTreeSet<_> = new_simplices.iter().copied().collect();
    if old_simplices.len() != new_simplices.len()
        || old_columns.len() != old_simplices.len()
        || old_set.len() != old_simplices.len()
        || old_set != new_set
    {
        return Err(ProofError::new(
            "reduction repair found a changed simplex set",
        ));
    }
    Ok(())
}

fn repair_candidates<const N: usize>(
    old_simplices: &[[usize; N]],
    new_simplices: &[[usize; N]],
    old_columns: &[ProofColumn],
    old_positions: &BTreeMap<[usize; N], usize>,
    new_positions: &BTreeMap<[usize; N], usize>,
) -> Result<Vec<ProofColumn>, ProofError> {
    let mut candidates = Vec::new();
    for (new_target, simplex) in new_simplices.iter().enumerate() {
        let old_target = old_positions[simplex];
        let Some(candidate) = repair_candidate(
            new_target,
            &old_columns[old_target],
            old_simplices,
            new_positions,
        )?
        else {
            break;
        };
        candidates.push(candidate);
    }
    Ok(candidates)
}

fn repair_candidate<const N: usize>(
    new_target: usize,
    old_column: &ProofColumn,
    old_simplices: &[[usize; N]],
    new_positions: &BTreeMap<[usize; N], usize>,
) -> Result<Option<ProofColumn>, ProofError> {
    let mut terms = Vec::with_capacity(old_column.terms.len());
    for term in &old_column.terms {
        let source = *old_simplices
            .get(term.index)
            .ok_or_else(|| ProofError::new("reduction repair found an invalid source position"))?;
        let index = new_positions[&source];
        if index > new_target {
            return Ok(None);
        }
        terms.push(ProofTerm {
            index,
            coefficient: term.coefficient,
        });
    }
    terms.sort_by_key(|term| term.index);
    if terms.windows(2).any(|pair| pair[0].index == pair[1].index) {
        return Err(ProofError::new(
            "reduction repair produced duplicate source positions",
        ));
    }
    Ok(Some(ProofColumn { terms }))
}
