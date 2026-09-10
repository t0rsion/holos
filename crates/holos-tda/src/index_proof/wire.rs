use crate::{Bar, ChangeColumn, InterfaceMode};

use super::{F64_BITS_CODEC, IndexProofError, ProofNode, VERSION};

pub(super) fn encode_header(
    output: &mut Vec<u8>,
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    vertices: usize,
) -> Result<(), IndexProofError> {
    put_u16(output, VERSION);
    output.push(F64_BITS_CODEC);
    put_usize(output, max_dim)?;
    put_u32(output, modulus);
    put_optional_f64(output, threshold);
    put_usize(output, vertices)?;
    Ok(())
}

pub(super) fn encode_nodes(
    output: &mut Vec<u8>,
    nodes: &[ProofNode],
) -> Result<(), IndexProofError> {
    for node in nodes {
        encode_node(output, node)?;
    }
    Ok(())
}

fn encode_node(output: &mut Vec<u8>, node: &ProofNode) -> Result<(), IndexProofError> {
    encode_node_header(output, node)?;
    encode_usizes(output, &node.vertices)?;
    encode_usizes(output, &node.edge_positions)?;
    encode_usizes(output, &node.separator)?;
    encode_usizes(output, &node.protected_vertices)?;
    encode_digests(output, &node.children);
    encode_graded_columns(output, &node.graded_columns)?;
    encode_diagram(output, &node.diagram)?;
    output.extend_from_slice(&node.relative_artifact);
    Ok(())
}

fn encode_node_header(output: &mut Vec<u8>, node: &ProofNode) -> Result<(), IndexProofError> {
    output.extend_from_slice(&node.digest);
    output.push(interface_mode_tag(node.mode));
    encode_node_counts(output, node)?;
    for columns in &node.graded_columns {
        put_usize(output, columns.len())?;
    }
    put_usize(output, node.diagram.len())?;
    put_usize(output, node.relative_artifact.len())?;
    Ok(())
}

fn interface_mode_tag(mode: InterfaceMode) -> u8 {
    match mode {
        InterfaceMode::Relative => 4,
        InterfaceMode::Materialized => 0,
        InterfaceMode::Disjoint => 1,
        InterfaceMode::ZeroSimplex => 2,
        InterfaceMode::ZeroCone => 3,
    }
}

fn encode_node_counts(output: &mut Vec<u8>, node: &ProofNode) -> Result<(), IndexProofError> {
    put_usize(output, node.vertices.len())?;
    put_usize(output, node.edge_positions.len())?;
    put_usize(output, node.separator.len())?;
    put_usize(output, node.protected_vertices.len())?;
    put_usize(output, node.children.len())?;
    put_usize(output, node.graded_columns.len())?;
    Ok(())
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<(), IndexProofError> {
    for &value in values {
        put_usize(output, value)?;
    }
    Ok(())
}

fn encode_digests(output: &mut Vec<u8>, digests: &[[u8; 32]]) {
    for digest in digests {
        output.extend_from_slice(digest);
    }
}

fn encode_graded_columns(
    output: &mut Vec<u8>,
    dimensions: &[Vec<ChangeColumn>],
) -> Result<(), IndexProofError> {
    for columns in dimensions {
        encode_columns(output, columns)?;
    }
    Ok(())
}

fn encode_columns(output: &mut Vec<u8>, columns: &[ChangeColumn]) -> Result<(), IndexProofError> {
    for column in columns {
        put_usize(output, column.terms.len())?;
        for term in &column.terms {
            put_usize(output, term.index)?;
            put_u32(output, term.coefficient);
        }
    }
    Ok(())
}

pub(super) fn encode_diagram(output: &mut Vec<u8>, diagram: &[Bar]) -> Result<(), IndexProofError> {
    for bar in diagram {
        put_usize(output, bar.dim)?;
        put_u64(output, bar.birth.to_bits());
        put_u64(output, bar.death.to_bits());
    }
    Ok(())
}

fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

pub(super) fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), IndexProofError> {
    let value = u64::try_from(value)
        .map_err(|_| IndexProofError::new("integer does not fit the proof format"))?;
    put_u64(output, value);
    Ok(())
}

fn put_optional_f64(output: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => output.push(0),
        Some(value) => {
            output.push(1);
            put_u64(output, value.to_bits());
        }
    }
}
