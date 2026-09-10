use super::super::{Graph, ProofError, ProofLimits, checked_threshold};
use super::decode::decode_verified;
use super::digest::{cell_order, dimension_limit};
use super::model::{Cell, IndexLeafContext, Term, VerifiedCertificate};

pub(crate) fn verify_index_leaf(
    bytes: &[u8],
    context: IndexLeafContext<'_>,
) -> Result<VerifiedCertificate, ProofError> {
    let certificate = decode_verified(bytes, context.limits)?;
    if certificate.max_dim != context.max_dim
        || certificate.modulus != context.modulus
        || certificate.protected_vertices != context.protected_vertices
    {
        return Err(ProofError::new(
            "relative leaf changes the dimension, field, or protected vertices",
        ));
    }
    let expected = enumerate_flag_cells(
        context.graph,
        context.labels,
        context.threshold,
        context.max_dim,
        context.modulus,
        context.limits,
    )?;
    if certificate.input != expected {
        let dimension = certificate
            .input
            .iter()
            .zip(&expected)
            .position(|(actual, expected)| actual != expected)
            .unwrap_or(0);
        return Err(ProofError::new(format!(
            "relative leaf input differs from its induced filtered flag complex in dimension {dimension}: certificate has {:?}, graph has {:?}",
            certificate.input[dimension], expected[dimension]
        )));
    }
    Ok(certificate)
}

pub(super) fn enumerate_flag_cells(
    graph: &Graph,
    labels: &[usize],
    threshold: Option<f64>,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
) -> Result<Vec<Vec<Cell>>, ProofError> {
    let threshold = checked_threshold(threshold)?;
    if labels.len() > limits.max_vertices {
        return Err(ProofError::new("relative leaf exceeds the vertex limit"));
    }
    let mut cells = vec![vertex_cells(labels)];
    for dimension in 1..=max_dim + 1 {
        let next = enumerate_dimension(
            graph,
            labels,
            threshold,
            dimension,
            &cells[dimension - 1],
            dimension_limit(dimension, limits),
            modulus,
        )?;
        cells.push(next);
    }
    Ok(cells)
}

pub(super) fn vertex_cells(labels: &[usize]) -> Vec<Cell> {
    let mut vertices = labels
        .iter()
        .map(|&vertex| Cell {
            vertices: vec![vertex],
            value: 0.0,
            boundary: Vec::new(),
        })
        .collect::<Vec<_>>();
    vertices.sort_by(cell_order);
    vertices
}

pub(super) fn enumerate_dimension(
    graph: &Graph,
    labels: &[usize],
    threshold: f64,
    dimension: usize,
    previous: &[Cell],
    limit: usize,
    modulus: u32,
) -> Result<Vec<Cell>, ProofError> {
    let mut next = Vec::new();
    for simplex in previous {
        let start = labels
            .binary_search(simplex.vertices.last().expect("a simplex is nonempty"))
            .expect("the preceding simplex uses declared labels")
            + 1;
        for &vertex in &labels[start..] {
            if let Some(cell) = extend_simplex(graph, simplex, vertex, threshold, modulus) {
                if next.len() == limit {
                    return Err(ProofError::new(format!(
                        "relative leaf dimension {dimension} exceeds the simplex limit"
                    )));
                }
                next.push(cell);
            }
        }
    }
    next.sort_by(cell_order);
    Ok(next)
}

pub(super) fn extend_simplex(
    graph: &Graph,
    simplex: &Cell,
    vertex: usize,
    threshold: f64,
    modulus: u32,
) -> Option<Cell> {
    let value = simplex_value(graph, simplex, vertex, threshold)?;
    let mut vertices = simplex.vertices.clone();
    vertices.push(vertex);
    Some(Cell {
        boundary: simplex_boundary(&vertices, modulus),
        vertices,
        value,
    })
}

pub(super) fn simplex_value(
    graph: &Graph,
    simplex: &Cell,
    vertex: usize,
    threshold: f64,
) -> Option<f64> {
    let mut value = simplex.value;
    for &member in &simplex.vertices {
        let edge = graph.get(member, vertex);
        if !edge.is_finite() || edge > threshold {
            return None;
        }
        value = value.max(edge);
    }
    Some(value)
}

pub(super) fn simplex_boundary(vertices: &[usize], modulus: u32) -> Vec<Term> {
    let mut boundary = (0..vertices.len())
        .map(|removed| {
            let mut cell = vertices.to_vec();
            cell.remove(removed);
            Term {
                cell,
                coefficient: if removed % 2 == 0 { 1 } else { modulus - 1 },
            }
        })
        .collect::<Vec<_>>();
    boundary.sort();
    boundary
}
