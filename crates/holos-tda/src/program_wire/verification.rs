use sha2::{Digest, Sha256};

use crate::field::{MODULUS_LIMIT, is_prime};
use crate::program::{ProgramAtomState, local_matrix};
use crate::{Bar, CertificateLimits, Diagram, EdgeKey, SparseDistanceMatrix};

use super::model::{
    ProgramArtifact, ProgramArtifactError, ProgramAtomArtifact, ProgramDecodeLimits,
};

pub(super) fn check_input_binding(
    artifact: &ProgramArtifact,
    input: &SparseDistanceMatrix,
) -> Result<(), ProgramArtifactError> {
    let differs = input.len() != artifact.vertex_count
        || program_graph_digest(input, artifact.threshold) != artifact.input_digest;
    if differs {
        return Err(ProgramArtifactError::new(
            "complete input graph binding does not match",
        ));
    }
    Ok(())
}

pub(super) fn verify_atom_states(
    artifact: &ProgramArtifact,
    input: &SparseDistanceMatrix,
    infos: &[crate::ProgramAtomInfo],
    topology: &[EdgeKey],
    certificate_limits: CertificateLimits,
) -> Result<Vec<ProgramAtomState>, ProgramArtifactError> {
    let cyclic: Vec<_> = infos.iter().filter(|atom| atom.cyclic).collect();
    check_cyclic_atom_count(cyclic.len(), artifact.atoms.len())?;
    let mut states = Vec::with_capacity(artifact.atoms.len());
    for (record, expected) in artifact.atoms.iter().zip(cyclic) {
        states.push(verify_atom_state(
            record,
            expected,
            input,
            topology,
            certificate_limits,
        )?);
    }
    Ok(states)
}

fn check_cyclic_atom_count(actual: usize, recorded: usize) -> Result<(), ProgramArtifactError> {
    if actual != recorded {
        return Err(ProgramArtifactError::new(
            "cyclic atom count differs from the checked decomposition",
        ));
    }
    Ok(())
}

fn verify_atom_state(
    record: &ProgramAtomArtifact,
    expected: &crate::ProgramAtomInfo,
    input: &SparseDistanceMatrix,
    topology: &[EdgeKey],
    certificate_limits: CertificateLimits,
) -> Result<ProgramAtomState, ProgramArtifactError> {
    check_atom_decomposition(record, expected)?;
    let local = local_matrix(&record.vertices, &record.edges, input)
        .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
    record
        .atlas
        .verify(&local, certificate_limits)
        .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
    let region = record
        .atlas
        .reduction_certificate()
        .compile_region(&local, certificate_limits)
        .map_err(|error| ProgramArtifactError::new(error.to_string()))?;
    Ok(ProgramAtomState {
        info_index: record.id,
        vertices: record.vertices.clone(),
        edges: record.edges.clone(),
        edge_positions: atom_edge_positions(&record.edges, topology),
        artifact: record.atlas.clone(),
        certified_graph: local,
        region,
        explained: record.atlas.explained().clone(),
    })
}

fn check_atom_decomposition(
    record: &ProgramAtomArtifact,
    expected: &crate::ProgramAtomInfo,
) -> Result<(), ProgramArtifactError> {
    let differs = record.id != expected.id
        || record.vertices != expected.vertices
        || record.edges != expected.edges;
    if differs {
        return Err(ProgramArtifactError::new(format!(
            "atom {} differs from the checked decomposition",
            record.id
        )));
    }
    Ok(())
}

fn atom_edge_positions(edges: &[EdgeKey], topology: &[EdgeKey]) -> Vec<usize> {
    edges
        .iter()
        .map(|edge| {
            topology
                .binary_search(edge)
                .expect("checked atom edge is in the program topology")
        })
        .collect()
}

pub(super) fn check_composed_diagram(
    actual: &Diagram,
    recorded: &Diagram,
) -> Result<(), ProgramArtifactError> {
    if !diagram_bits_equal(actual, recorded) {
        return Err(ProgramArtifactError::new(
            "composed diagram differs from the recorded diagram",
        ));
    }
    Ok(())
}

pub(super) fn check_program_limits(
    artifact: &ProgramArtifact,
    limits: ProgramDecodeLimits,
) -> Result<(), ProgramArtifactError> {
    if !is_prime(u64::from(artifact.modulus)) || u64::from(artifact.modulus) >= MODULUS_LIMIT {
        return Err(ProgramArtifactError::new(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {}",
            artifact.modulus
        )));
    }
    if artifact.vertex_count > limits.max_vertices {
        return Err(ProgramArtifactError::new(format!(
            "{} vertices exceed the limit {}",
            artifact.vertex_count, limits.max_vertices
        )));
    }
    checked_threshold(artifact.threshold)?;
    if artifact.atoms.len() > limits.max_atoms {
        return Err(ProgramArtifactError::new(format!(
            "{} atoms exceed the limit {}",
            artifact.atoms.len(),
            limits.max_atoms
        )));
    }
    Ok(())
}

pub(super) fn check_program_diagram(diagram: &Diagram) -> Result<(), ProgramArtifactError> {
    let mut canonical = diagram.clone();
    canonical.canonicalize();
    if !diagram_bits_equal(&canonical, diagram) {
        return Err(ProgramArtifactError::new("bars are not in canonical order"));
    }
    for bar in &diagram.bars {
        check_program_bar(bar)?;
    }
    Ok(())
}

fn check_program_bar(bar: &Bar) -> Result<(), ProgramArtifactError> {
    let invalid = bar.dim > 1
        || !bar.birth.is_finite()
        || bar.birth < 0.0
        || is_negative_zero(bar.birth)
        || bar.death.is_nan()
        || bar.death < 0.0
        || is_negative_zero(bar.death)
        || bar.death <= bar.birth;
    if invalid {
        return Err(ProgramArtifactError::new("diagram contains an invalid bar"));
    }
    Ok(())
}

pub(super) fn check_program_atoms(
    artifact: &ProgramArtifact,
    limits: ProgramDecodeLimits,
) -> Result<(), ProgramArtifactError> {
    let mut total_vertices = 0usize;
    let mut total_edges = 0usize;
    let mut previous_id = None;
    for atom in &artifact.atoms {
        check_atom_id(previous_id, atom.id)?;
        previous_id = Some(atom.id);
        total_vertices = bounded_sum(
            total_vertices,
            atom.vertices.len(),
            limits.max_atom_vertices,
            "atom vertices",
        )?;
        total_edges = bounded_sum(
            total_edges,
            atom.edges.len(),
            limits.max_atom_edges,
            "atom edges",
        )?;
        check_atom_vertices(atom, artifact.vertex_count)?;
        check_atom_edges(atom)?;
        check_atom_atlas(atom, artifact)?;
    }
    Ok(())
}

fn check_atom_id(previous: Option<usize>, id: usize) -> Result<(), ProgramArtifactError> {
    if previous.is_some_and(|previous| previous >= id) {
        return Err(ProgramArtifactError::new(
            "atom identifiers are not strictly ordered",
        ));
    }
    Ok(())
}

fn check_atom_vertices(
    atom: &ProgramAtomArtifact,
    vertex_count: usize,
) -> Result<(), ProgramArtifactError> {
    let invalid = atom.vertices.is_empty()
        || !atom.vertices.windows(2).all(|pair| pair[0] < pair[1])
        || atom.vertices.iter().any(|&vertex| vertex >= vertex_count);
    if invalid {
        return Err(ProgramArtifactError::new(format!(
            "atom {} has noncanonical vertices",
            atom.id
        )));
    }
    Ok(())
}

fn check_atom_edges(atom: &ProgramAtomArtifact) -> Result<(), ProgramArtifactError> {
    let invalid = atom.edges.len() < atom.vertices.len()
        || !atom.edges.windows(2).all(|pair| pair[0] < pair[1])
        || atom
            .edges
            .iter()
            .any(|edge| !atom_edge_is_canonical(atom, edge));
    if invalid {
        return Err(ProgramArtifactError::new(format!(
            "atom {} has noncanonical edges",
            atom.id
        )));
    }
    Ok(())
}

fn atom_edge_is_canonical(atom: &ProgramAtomArtifact, edge: &EdgeKey) -> bool {
    edge.u < edge.v
        && atom.vertices.binary_search(&edge.u).is_ok()
        && atom.vertices.binary_search(&edge.v).is_ok()
}

fn check_atom_atlas(
    atom: &ProgramAtomArtifact,
    artifact: &ProgramArtifact,
) -> Result<(), ProgramArtifactError> {
    let differs = atom.atlas.vertex_count() != atom.vertices.len()
        || atom.atlas.threshold().map(f64::to_bits) != artifact.threshold.map(f64::to_bits)
        || atom.atlas.modulus() != artifact.modulus;
    if differs {
        return Err(ProgramArtifactError::new(format!(
            "atom {} atlas header differs from the program",
            atom.id
        )));
    }
    Ok(())
}

pub(super) fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> std::result::Result<usize, ProgramArtifactError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProgramArtifactError::new(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(ProgramArtifactError::new(format!(
            "{next} {label} exceed the limit {limit}"
        )));
    }
    Ok(next)
}

fn checked_threshold(threshold: Option<f64>) -> std::result::Result<f64, ProgramArtifactError> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 || is_negative_zero(threshold) {
        return Err(ProgramArtifactError::new(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

pub(super) fn program_graph_digest(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-program-graph-v1");
    hash.update((input.len() as u64).to_be_bytes());
    match threshold {
        None => hash.update([0]),
        Some(value) => {
            hash.update([1]);
            hash.update(value.to_bits().to_be_bytes());
        }
    }
    hash.update((input.num_edges() as u64).to_be_bytes());
    for (u, v, value) in input.edges() {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

pub(super) fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}
