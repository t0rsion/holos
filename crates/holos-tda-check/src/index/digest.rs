use sha2::{Digest, Sha256};

use crate::relative::decode_verified;
use crate::{Graph, ProofBar, ProofColumn, ProofError, ProofLimits};

use super::model::{InterfaceMode, InterfaceProof};
use super::scope::local_graph;

pub(super) fn interface_digest(
    node: &InterfaceProof,
    graph: &Graph,
    threshold: Option<f64>,
    diagram: &[ProofBar],
    limits: ProofLimits,
) -> Result<[u8; 32], ProofError> {
    let mut hash = Sha256::new();
    hash.update(b"holos-filtered-interface-v4");
    digest_usizes(&mut hash, &node.vertices);
    digest_usizes(&mut hash, &node.edge_positions);
    digest_usizes(&mut hash, &node.separator);
    digest_usizes(&mut hash, &node.protected_vertices);
    hash.update((node.children.len() as u64).to_be_bytes());
    for child in &node.children {
        hash.update(child);
    }
    digest_interface_mode(&mut hash, node, graph, threshold, limits)?;
    digest_diagram(&mut hash, diagram);
    Ok(hash.finalize().into())
}

fn digest_interface_mode(
    hash: &mut Sha256,
    node: &InterfaceProof,
    graph: &Graph,
    threshold: Option<f64>,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    match node.mode {
        InterfaceMode::Relative => {
            let relative = decode_verified(&node.relative_artifact, limits)?;
            hash.update([4]);
            hash.update(relative.source_digest());
            hash.update(relative.digest);
        }
        InterfaceMode::Materialized => {
            hash.update([0]);
            let local = local_graph(graph, node)?;
            hash.update(filtered_graph_digest(&local, threshold));
            hash.update(((node.graded_columns.len() - 1) as u64).to_be_bytes());
            hash.update((node.graded_columns.len() as u64).to_be_bytes());
            for columns in &node.graded_columns {
                digest_columns(hash, columns);
            }
        }
        InterfaceMode::Disjoint => hash.update([1]),
        InterfaceMode::ZeroSimplex => hash.update([2]),
        InterfaceMode::ZeroCone => hash.update([3]),
    }
    Ok(())
}

fn digest_diagram(hash: &mut Sha256, diagram: &[ProofBar]) {
    hash.update((diagram.len() as u64).to_be_bytes());
    for bar in diagram {
        hash.update((bar.dimension as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
}

fn filtered_graph_digest(graph: &Graph, threshold: Option<f64>) -> [u8; 32] {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-certified-graph-v1");
    hash.update((graph.vertex_count as u64).to_be_bytes());
    hash.update((edges.len() as u64).to_be_bytes());
    for edge in edges {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(edge.value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for &value in values {
        hash.update((value as u64).to_be_bytes());
    }
}

fn digest_columns(hash: &mut Sha256, columns: &[ProofColumn]) {
    hash.update((columns.len() as u64).to_be_bytes());
    for column in columns {
        hash.update((column.terms.len() as u64).to_be_bytes());
        for term in &column.terms {
            hash.update((term.index as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
}
