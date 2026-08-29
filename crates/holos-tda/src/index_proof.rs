//! Cold snapshots and warm deltas for versioned persistence indexes.
//!
//! A snapshot contains one complete interface tree. A delta contains only
//! changed edge values and interface nodes absent from the preceding tree.
//! The separate checker keeps the verified old tree and advances its root.

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use crate::index::InterfaceNode;
use crate::{Bar, CertificateLimits, ChangeColumn, InterfaceMode, PersistenceIndex};

const SNAPSHOT_MAGIC: &[u8; 8] = b"HOLOSIP\0";
const DELTA_MAGIC: &[u8; 8] = b"HOLOSDP\0";
const VERSION: u16 = 4;
const F64_BITS_CODEC: u8 = 1;

/// Failure while producing or encoding an index proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexProofError {
    message: String,
}

impl IndexProofError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated producer rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for IndexProofError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "index proof: {}", self.message)
    }
}

impl std::error::Error for IndexProofError {}

/// Structural size of a cold snapshot or warm delta.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexProofSummary {
    /// Interface nodes carried by this record.
    pub nodes: usize,
    /// Carried nodes checked by separator composition.
    pub composed_nodes: usize,
    /// Carried nodes checked as relative filtered cores.
    pub relative_nodes: usize,
    /// Encoded relative-interface bytes carried by this record.
    pub relative_bytes: usize,
    /// Edge values changed by this record.
    pub edge_changes: usize,
    /// Edge-boundary change columns carried by this record.
    pub edge_columns: usize,
    /// Triangle-boundary change columns carried by this record.
    pub triangle_columns: usize,
    /// Change columns above the triangle boundary carried by this record.
    pub higher_columns: usize,
    /// Sparse change-of-basis terms carried by this record.
    pub terms: usize,
}

#[derive(Debug, Clone)]
struct ProofNode {
    digest: [u8; 32],
    vertices: Vec<usize>,
    edge_positions: Vec<usize>,
    separator: Vec<usize>,
    protected_vertices: Vec<usize>,
    children: Vec<[u8; 32]>,
    mode: InterfaceMode,
    graded_columns: Vec<Vec<ChangeColumn>>,
    relative_artifact: Vec<u8>,
    relative_column_counts: Vec<usize>,
    relative_terms: usize,
    diagram: Vec<Bar>,
}

impl ProofNode {
    fn from_interface(
        node: &InterfaceNode,
        max_dim: usize,
        limits: CertificateLimits,
    ) -> Result<Self, IndexProofError> {
        let graded_columns = node
            .reduction()
            .map(|reduction| reduction.graded_columns().to_vec())
            .unwrap_or_else(|| vec![Vec::new(); max_dim + 1]);
        let relative_artifact = node
            .relative()
            .map(|relative| relative.encode(limits))
            .transpose()
            .map_err(|error| IndexProofError::new(error.to_string()))?
            .unwrap_or_default();
        let relative_column_counts = node
            .relative()
            .map(|relative| relative.graded_columns().iter().map(Vec::len).collect())
            .unwrap_or_default();
        let relative_terms = node
            .relative()
            .map(|relative| {
                relative
                    .graded_columns()
                    .iter()
                    .flatten()
                    .map(|column| column.terms.len())
                    .sum()
            })
            .unwrap_or(0);
        Ok(Self {
            digest: node.digest,
            vertices: node.vertices.clone(),
            edge_positions: node.edge_positions.clone(),
            separator: node.separator.clone(),
            protected_vertices: node.protected_vertices.clone(),
            children: node.children.iter().map(|child| child.digest).collect(),
            mode: node.mode(),
            graded_columns,
            relative_artifact,
            relative_column_counts,
            relative_terms,
            diagram: node.diagram().bars.clone(),
        })
    }

    fn summary(&self, summary: &mut IndexProofSummary) {
        summary.nodes += 1;
        if self.mode != InterfaceMode::Materialized {
            summary.composed_nodes += 1;
        }
        if self.mode == InterfaceMode::Relative {
            summary.relative_nodes += 1;
            summary.relative_bytes += self.relative_artifact.len();
        }
        summary.edge_columns += self.graded_columns.first().map_or(0, Vec::len);
        summary.edge_columns += self.relative_column_counts.first().copied().unwrap_or(0);
        summary.triangle_columns += self.graded_columns.get(1).map_or(0, Vec::len);
        summary.triangle_columns += self.relative_column_counts.get(1).copied().unwrap_or(0);
        summary.higher_columns += self
            .graded_columns
            .iter()
            .skip(2)
            .map(Vec::len)
            .sum::<usize>();
        summary.higher_columns += self.relative_column_counts.iter().skip(2).sum::<usize>();
        summary.terms += self
            .graded_columns
            .iter()
            .flatten()
            .map(|column| column.terms.len())
            .sum::<usize>();
        summary.terms += self.relative_terms;
    }
}

/// One complete proof-carrying persistence-index snapshot.
#[derive(Debug, Clone)]
pub struct IndexSnapshotProof {
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    vertex_count: usize,
    edges: Vec<(usize, usize, f64)>,
    root: [u8; 32],
    nodes: Vec<ProofNode>,
    diagram: Vec<Bar>,
}

impl IndexSnapshotProof {
    /// Capture a complete index version.
    pub fn from_index(index: &PersistenceIndex) -> Result<Self, IndexProofError> {
        let mut nodes = Vec::new();
        let mut seen = BTreeSet::new();
        collect_nodes(
            index.root(),
            index.params().max_dim,
            index.certificate_limits(),
            &mut seen,
            &mut nodes,
        )?;
        Ok(Self {
            max_dim: index.params().max_dim,
            modulus: index.params().modulus,
            threshold: index.params().threshold,
            vertex_count: index.graph().len(),
            edges: index.graph().edges().collect(),
            root: index.version(),
            nodes,
            diagram: index.diagram().bars.clone(),
        })
    }

    /// Root content identifier checked by a warm delta.
    pub fn root(&self) -> &[u8; 32] {
        &self.root
    }

    /// Structural size of this cold snapshot.
    pub fn summary(&self) -> IndexProofSummary {
        proof_summary(&self.nodes, 0)
    }

    /// Encode the canonical `HOLOSIP` version 4 snapshot.
    pub fn encode(&self) -> Result<Vec<u8>, IndexProofError> {
        let mut output = Vec::new();
        output.extend_from_slice(SNAPSHOT_MAGIC);
        encode_header(
            &mut output,
            self.max_dim,
            self.modulus,
            self.threshold,
            self.vertex_count,
        )?;
        put_usize(&mut output, self.edges.len())?;
        put_usize(&mut output, self.nodes.len())?;
        put_usize(&mut output, self.diagram.len())?;
        output.extend_from_slice(&self.root);
        for &(u, v, value) in &self.edges {
            put_usize(&mut output, u)?;
            put_usize(&mut output, v)?;
            put_u64(&mut output, value.to_bits());
        }
        encode_nodes(&mut output, &self.nodes)?;
        encode_diagram(&mut output, &self.diagram)?;
        Ok(output)
    }
}

/// One stateful proof delta between indexes with the same envelope.
#[derive(Debug, Clone)]
pub struct IndexDeltaProof {
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    vertex_count: usize,
    edge_count: usize,
    old_root: [u8; 32],
    new_root: [u8; 32],
    edge_changes: Vec<(usize, f64)>,
    nodes: Vec<ProofNode>,
    diagram: Vec<Bar>,
}

impl IndexDeltaProof {
    /// Produce a warm proof delta between two versions of one envelope.
    pub fn between(
        old: &PersistenceIndex,
        new: &PersistenceIndex,
    ) -> Result<Self, IndexProofError> {
        if old.params().max_dim != new.params().max_dim
            || old.params().modulus != new.params().modulus
            || old.params().threshold.map(f64::to_bits) != new.params().threshold.map(f64::to_bits)
            || old.graph().len() != new.graph().len()
            || old.topology() != new.topology()
        {
            return Err(IndexProofError::new(
                "a proof delta requires one field, threshold, and listed-edge envelope",
            ));
        }
        let edge_changes = old
            .topology()
            .iter()
            .enumerate()
            .filter_map(|(position, edge)| {
                let old_value = old.graph().get(edge.u, edge.v);
                let new_value = new.graph().get(edge.u, edge.v);
                (old_value.to_bits() != new_value.to_bits()).then_some((position, new_value))
            })
            .collect();
        let mut old_nodes = BTreeSet::new();
        collect_digests(old.root(), &mut old_nodes);
        let mut nodes = Vec::new();
        let mut seen = BTreeSet::new();
        collect_new_nodes(
            new.root(),
            new.params().max_dim,
            new.certificate_limits(),
            &old_nodes,
            &mut seen,
            &mut nodes,
        )?;
        Ok(Self {
            max_dim: old.params().max_dim,
            modulus: old.params().modulus,
            threshold: old.params().threshold,
            vertex_count: old.graph().len(),
            edge_count: old.topology().len(),
            old_root: old.version(),
            new_root: new.version(),
            edge_changes,
            nodes,
            diagram: new.diagram().bars.clone(),
        })
    }

    /// Root required before this delta can be applied.
    pub fn old_root(&self) -> &[u8; 32] {
        &self.old_root
    }

    /// Root established after this delta is verified.
    pub fn new_root(&self) -> &[u8; 32] {
        &self.new_root
    }

    /// Structural size of this warm delta.
    pub fn summary(&self) -> IndexProofSummary {
        proof_summary(&self.nodes, self.edge_changes.len())
    }

    /// Encode the canonical `HOLOSDP` version 4 delta.
    pub fn encode(&self) -> Result<Vec<u8>, IndexProofError> {
        let mut output = Vec::new();
        output.extend_from_slice(DELTA_MAGIC);
        encode_header(
            &mut output,
            self.max_dim,
            self.modulus,
            self.threshold,
            self.vertex_count,
        )?;
        put_usize(&mut output, self.edge_count)?;
        put_usize(&mut output, self.edge_changes.len())?;
        put_usize(&mut output, self.nodes.len())?;
        put_usize(&mut output, self.diagram.len())?;
        output.extend_from_slice(&self.old_root);
        output.extend_from_slice(&self.new_root);
        for &(position, value) in &self.edge_changes {
            put_usize(&mut output, position)?;
            put_u64(&mut output, value.to_bits());
        }
        encode_nodes(&mut output, &self.nodes)?;
        encode_diagram(&mut output, &self.diagram)?;
        Ok(output)
    }
}

fn collect_nodes(
    node: &Arc<InterfaceNode>,
    max_dim: usize,
    limits: CertificateLimits,
    seen: &mut BTreeSet<[u8; 32]>,
    output: &mut Vec<ProofNode>,
) -> Result<(), IndexProofError> {
    if !seen.insert(node.digest) {
        return Ok(());
    }
    output.push(ProofNode::from_interface(node, max_dim, limits)?);
    for child in &node.children {
        collect_nodes(child, max_dim, limits, seen, output)?;
    }
    Ok(())
}

fn collect_new_nodes(
    node: &Arc<InterfaceNode>,
    max_dim: usize,
    limits: CertificateLimits,
    old: &BTreeSet<[u8; 32]>,
    seen: &mut BTreeSet<[u8; 32]>,
    output: &mut Vec<ProofNode>,
) -> Result<(), IndexProofError> {
    if old.contains(&node.digest) || !seen.insert(node.digest) {
        return Ok(());
    }
    output.push(ProofNode::from_interface(node, max_dim, limits)?);
    for child in &node.children {
        collect_new_nodes(child, max_dim, limits, old, seen, output)?;
    }
    Ok(())
}

fn collect_digests(node: &Arc<InterfaceNode>, output: &mut BTreeSet<[u8; 32]>) {
    if !output.insert(node.digest) {
        return;
    }
    for child in &node.children {
        collect_digests(child, output);
    }
}

fn proof_summary(nodes: &[ProofNode], edge_changes: usize) -> IndexProofSummary {
    let mut summary = IndexProofSummary {
        edge_changes,
        ..IndexProofSummary::default()
    };
    for node in nodes {
        node.summary(&mut summary);
    }
    summary
}

fn encode_header(
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

fn encode_nodes(output: &mut Vec<u8>, nodes: &[ProofNode]) -> Result<(), IndexProofError> {
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

fn encode_diagram(output: &mut Vec<u8>, diagram: &[Bar]) -> Result<(), IndexProofError> {
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

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), IndexProofError> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CertificateLimits, IndexParams, RipsParams, SparseDistanceMatrix};

    fn graph(changed: bool) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 0.25),
                (0, 2, if changed { 1.01 } else { 1.0 }),
                (1, 2, 1.5),
                (0, 3, 1.2),
                (1, 3, 1.7),
                (0, 4, 1.1),
                (1, 4, 1.6),
                (0, 5, 1.3),
                (1, 5, 1.8),
            ],
        )
        .unwrap()
    }

    #[test]
    fn warm_delta_contains_only_changed_tree_nodes() {
        let initial = graph(false);
        let params = RipsParams::new(1).with_modulus(3);
        let first = PersistenceIndex::compile(
            &initial,
            &params,
            IndexParams::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        let second = first.transition(&graph(true)).unwrap().index;
        let snapshot = IndexSnapshotProof::from_index(&first).unwrap();
        let delta = IndexDeltaProof::between(&first, &second).unwrap();
        assert_eq!(delta.summary().edge_changes, 1);
        assert!(delta.summary().nodes < snapshot.summary().nodes);
        assert!(snapshot.encode().unwrap().starts_with(SNAPSHOT_MAGIC));
        assert!(delta.encode().unwrap().starts_with(DELTA_MAGIC));
    }

    #[test]
    fn delta_rejects_a_different_envelope() {
        let initial = graph(false);
        let params = RipsParams::new(1);
        let first = PersistenceIndex::compile(
            &initial,
            &params,
            IndexParams::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        let other_graph = SparseDistanceMatrix::from_triplets(2, &[(0, 1, 1.0)]).unwrap();
        let second = PersistenceIndex::compile(
            &other_graph,
            &params,
            IndexParams::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert!(IndexDeltaProof::between(&first, &second).is_err());
    }
}
