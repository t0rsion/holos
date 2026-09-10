use super::super::CollapseCertificate;
use super::model::{ArtifactError, CollapseArtifact};
use super::primitives::graph_digest;
use crate::SparseDistanceMatrix;

pub(super) fn validate_stored_bindings(
    artifact: &CollapseArtifact,
    output: &[(usize, usize, f64)],
) -> Result<(), ArtifactError> {
    let input = reconstruct_input(output, &artifact.certificate)?;
    let vertices = artifact.certificate.vertex_count();
    let input_digest = graph_digest(vertices, &input);
    let output_digest = graph_digest(vertices, output);
    if input_digest != artifact.input_digest || output_digest != artifact.output_digest {
        Err(ArtifactError::new(
            "stored graph binding does not match the certificate and output graph",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn canonical_output(
    matrix: &SparseDistanceMatrix,
) -> Result<Vec<(usize, usize, f64)>, ArtifactError> {
    let edges: Vec<_> = matrix.edges().collect();
    validate_edges(matrix.len(), &edges, "output")?;
    Ok(edges)
}

pub(super) fn reconstruct_input(
    output: &[(usize, usize, f64)],
    certificate: &CollapseCertificate,
) -> Result<Vec<(usize, usize, f64)>, ArtifactError> {
    let mut input = Vec::with_capacity(output.len().saturating_add(certificate.steps().len()));
    input.extend_from_slice(output);
    input.extend(certificate.steps().iter().map(|step| {
        let (u, v) = step.edge();
        (u, v, step.value())
    }));
    input.sort_unstable_by_key(|edge| (edge.0, edge.1));
    validate_edges(certificate.vertex_count(), &input, "reconstructed input")?;
    if input.len() != certificate.input_edge_count() {
        return Err(ArtifactError::new(format!(
            "certificate records {} input edges, reconstruction has {}",
            certificate.input_edge_count(),
            input.len()
        )));
    }
    Ok(input)
}

pub(super) fn validate_edges(
    vertices: usize,
    edges: &[(usize, usize, f64)],
    label: &str,
) -> Result<(), ArtifactError> {
    let mut previous = None;
    for (index, &(u, v, value)) in edges.iter().enumerate() {
        validate_edge(vertices, index, (u, v, value), label)?;
        validate_edge_order(previous, (u, v), label)?;
        previous = Some((u, v));
    }
    Ok(())
}

fn validate_edge(
    vertices: usize,
    index: usize,
    edge: (usize, usize, f64),
    label: &str,
) -> Result<(), ArtifactError> {
    let (u, v, value) = edge;
    if u >= v || v >= vertices {
        return Err(ArtifactError::new(format!(
            "{label} edge {index} has invalid endpoints ({u}, {v}) for {vertices} vertices"
        )));
    }
    if !value.is_finite() || value < 0.0 {
        return Err(ArtifactError::new(format!(
            "{label} edge ({u}, {v}) has invalid value {value}"
        )));
    }
    if value == 0.0 && value.to_bits() != 0 {
        return Err(ArtifactError::new(format!(
            "{label} edge ({u}, {v}) encodes negative zero"
        )));
    }
    Ok(())
}

fn validate_edge_order(
    previous: Option<(usize, usize)>,
    current: (usize, usize),
    label: &str,
) -> Result<(), ArtifactError> {
    match previous.map(|edge| edge.cmp(&current)) {
        Some(std::cmp::Ordering::Equal) => Err(ArtifactError::new(format!(
            "{label} repeats edge ({}, {})",
            current.0, current.1
        ))),
        Some(std::cmp::Ordering::Greater) => Err(ArtifactError::new(format!(
            "{label} edges are not in ascending endpoint order"
        ))),
        _ => Ok(()),
    }
}
