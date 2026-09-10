use std::collections::BTreeMap;

use crate::ProofError;

use super::super::model::{ProofBar, ProofColumn, ProofTerm};
use super::diagram::canonicalize_diagram;
use super::model::{FilteredComplex, Graph, SparseColumn};

pub(crate) struct CheckedReduction {
    pub(crate) diagram: Vec<ProofBar>,
}

pub(crate) fn check_reduction(
    graph: &Graph,
    threshold: Option<f64>,
    modulus: u32,
    edge_columns: &[ProofColumn],
    triangle_columns: &[ProofColumn],
) -> Result<CheckedReduction, ProofError> {
    let complex = FilteredComplex::build(graph, threshold)?;
    let reduced_edges = check_matrix(
        &complex.edge_boundaries(modulus),
        edge_columns,
        modulus,
        "edge",
    )?;
    let reduced_triangles = check_matrix(
        &complex.triangle_boundaries(modulus),
        triangle_columns,
        modulus,
        "triangle",
    )?;
    let mut diagram = h0_from_reduction(&complex, &reduced_edges);
    diagram.extend(h1_from_reduction(
        &complex,
        &reduced_edges,
        &reduced_triangles,
    ));
    canonicalize_diagram(&mut diagram);
    Ok(CheckedReduction { diagram })
}

fn h0_from_reduction(complex: &FilteredComplex, reduced_edges: &[SparseColumn]) -> Vec<ProofBar> {
    let mut diagram = Vec::new();
    let mut killed_vertices = vec![false; complex.vertex_count];
    for (position, reduced) in reduced_edges.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            killed_vertices[pivot] = true;
            let death = complex.edges[position].value;
            if death > 0.0 {
                diagram.push(ProofBar {
                    dimension: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
    }
    diagram.extend(
        killed_vertices
            .into_iter()
            .filter(|&killed| !killed)
            .map(|_| ProofBar {
                dimension: 0,
                birth: 0.0,
                death: f64::INFINITY,
            }),
    );
    diagram
}

fn h1_from_reduction(
    complex: &FilteredComplex,
    reduced_edges: &[SparseColumn],
    reduced_triangles: &[SparseColumn],
) -> Vec<ProofBar> {
    let mut diagram = Vec::new();
    let mut deaths = BTreeMap::new();
    for (position, reduced) in reduced_triangles.iter().enumerate() {
        if let Some((pivot, _)) = reduced.pivot() {
            deaths.insert(pivot, complex.triangles[position].value);
        }
    }
    for (edge, reduced) in reduced_edges.iter().enumerate() {
        if !reduced.0.is_empty() {
            continue;
        }
        let birth = complex.edges[edge].value;
        let death = deaths.get(&edge).copied().unwrap_or(f64::INFINITY);
        if death > birth {
            diagram.push(ProofBar {
                dimension: 1,
                birth,
                death,
            });
        }
    }
    diagram
}

pub(crate) fn check_matrix(
    boundaries: &[SparseColumn],
    columns: &[ProofColumn],
    modulus: u32,
    label: &str,
) -> Result<Vec<SparseColumn>, ProofError> {
    if boundaries.len() != columns.len() {
        return Err(ProofError::new(format!(
            "{label} matrix has {} columns but proof records {}",
            boundaries.len(),
            columns.len()
        )));
    }
    let modulus64 = modulus as u64;
    let mut reduced = Vec::with_capacity(columns.len());
    let mut pivots = BTreeMap::new();
    for (target, transform) in columns.iter().enumerate() {
        check_column(target, &transform.terms, modulus)?;
        let mut result = SparseColumn::default();
        for term in &transform.terms {
            result.add_scaled(&boundaries[term.index], term.coefficient as u64, modulus64);
        }
        if let Some((pivot, _)) = result.pivot() {
            if let Some(previous) = pivots.insert(pivot, target) {
                return Err(ProofError::new(format!(
                    "{label} columns {previous} and {target} share pivot {pivot}"
                )));
            }
        }
        reduced.push(result);
    }
    Ok(reduced)
}

pub(crate) fn check_columns(columns: &[ProofColumn], modulus: u32) -> Result<(), ProofError> {
    for (target, column) in columns.iter().enumerate() {
        check_column(target, &column.terms, modulus)?;
    }
    Ok(())
}

pub(crate) fn check_column(
    target: usize,
    terms: &[ProofTerm],
    modulus: u32,
) -> Result<(), ProofError> {
    if terms.is_empty()
        || terms.last().map(|term| (term.index, term.coefficient)) != Some((target, 1))
    {
        return Err(ProofError::new("change column is not unit triangular"));
    }
    let mut previous = None;
    for term in terms {
        if term.index > target
            || term.coefficient == 0
            || term.coefficient >= modulus
            || previous.is_some_and(|position| position >= term.index)
        {
            return Err(ProofError::new("change column is not canonical"));
        }
        previous = Some(term.index);
    }
    Ok(())
}
