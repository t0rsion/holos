//! Unified proof DAGs for checked persistence trajectories.
//!
//! `HOLOSPF` stores unique local `D V = R` nodes once and references them
//! from each graph snapshot. The separate `holos-tda-check` crate and
//! `holos-check` binary decode and verify this format without depending on
//! the persistence solver.

use std::collections::BTreeMap;
use std::fmt;

use sha2::{Digest, Sha256};

use crate::{
    Bar, CertificateLimits, ChangeColumn, CorrespondenceMode, PersistenceProgram, RipsParams,
    SparseDistanceMatrix,
};

const MAGIC: &[u8; 8] = b"HOLOSPF\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Failure while producing or encoding a unified proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofArtifactError {
    message: String,
}

impl ProofArtifactError {
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

impl fmt::Display for ProofArtifactError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "unified proof: {}", self.message)
    }
}

impl std::error::Error for ProofArtifactError {}

#[derive(Debug, Clone)]
struct ProofNode {
    digest: [u8; 32],
    vertices: Vec<usize>,
    edges: Vec<[usize; 2]>,
    edge_columns: Vec<ChangeColumn>,
    triangle_columns: Vec<ChangeColumn>,
}

impl ProofNode {
    fn from_state(state: &crate::program::ProgramAtomState) -> Self {
        let mut node = Self {
            digest: [0; 32],
            vertices: state.vertices.clone(),
            edges: state.edges.iter().map(|edge| [edge.u, edge.v]).collect(),
            edge_columns: state
                .artifact
                .reduction_certificate()
                .edge_columns()
                .to_vec(),
            triangle_columns: state
                .artifact
                .reduction_certificate()
                .triangle_columns()
                .to_vec(),
        };
        node.digest = node.compute_digest();
        node
    }

    fn compute_digest(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"holos-proof-atom-v1");
        hash.update((self.vertices.len() as u64).to_be_bytes());
        for &vertex in &self.vertices {
            hash.update((vertex as u64).to_be_bytes());
        }
        hash.update((self.edges.len() as u64).to_be_bytes());
        for &[u, v] in &self.edges {
            hash.update((u as u64).to_be_bytes());
            hash.update((v as u64).to_be_bytes());
        }
        digest_columns(&mut hash, &self.edge_columns);
        digest_columns(&mut hash, &self.triangle_columns);
        hash.finalize().into()
    }
}

#[derive(Debug, Clone)]
struct ProofSnapshot {
    graph: SparseDistanceMatrix,
    atom_refs: Vec<[u8; 32]>,
    diagram: Vec<Bar>,
}

/// Size and reuse counts for a unified proof DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProofArtifactSummary {
    /// Graph snapshots in the proof trajectory.
    pub snapshots: usize,
    /// Unique algebraic reduction nodes.
    pub unique_nodes: usize,
    /// Node references across all snapshots.
    pub node_references: usize,
    /// References also present in the preceding snapshot.
    pub reused_references: usize,
}

/// Proof DAG for a program and its update trajectory.
#[derive(Debug, Clone)]
pub struct ProofArtifact {
    modulus: u32,
    threshold: Option<f64>,
    nodes: Vec<ProofNode>,
    snapshots: Vec<ProofSnapshot>,
}

impl ProofArtifact {
    /// Compile an initial program and every requested update into one DAG.
    pub fn build(
        initial: &SparseDistanceMatrix,
        updates: &[SparseDistanceMatrix],
        params: &RipsParams,
        limits: CertificateLimits,
    ) -> Result<Self, ProofArtifactError> {
        let mut program = PersistenceProgram::compile(initial, params, limits)
            .map_err(|error| ProofArtifactError::new(error.to_string()))?;
        let mut builder = ProofBuilder::new(params.modulus, params.threshold);
        builder.push(&program)?;
        for graph in updates {
            program
                .advance_with(graph, CorrespondenceMode::Omit)
                .map_err(|error| ProofArtifactError::new(error.to_string()))?;
            builder.push(&program)?;
        }
        Ok(builder.finish())
    }

    /// Capture one already compiled program.
    pub fn from_program(program: &PersistenceProgram) -> Result<Self, ProofArtifactError> {
        let mut builder = ProofBuilder::new(program.params().modulus, program.params().threshold);
        builder.push(program)?;
        Ok(builder.finish())
    }

    /// Structural size and node-reuse counts.
    pub fn summary(&self) -> ProofArtifactSummary {
        let node_references = self
            .snapshots
            .iter()
            .map(|snapshot| snapshot.atom_refs.len())
            .sum();
        let mut previous = std::collections::BTreeSet::new();
        let mut reused_references = 0usize;
        for snapshot in &self.snapshots {
            let current: std::collections::BTreeSet<_> =
                snapshot.atom_refs.iter().copied().collect();
            reused_references += current.intersection(&previous).count();
            previous = current;
        }
        ProofArtifactSummary {
            snapshots: self.snapshots.len(),
            unique_nodes: self.nodes.len(),
            node_references,
            reused_references,
        }
    }

    /// Encode the canonical `HOLOSPF` version 1 envelope.
    pub fn encode(&self) -> Result<Vec<u8>, ProofArtifactError> {
        let mut output = Vec::new();
        encode_proof_header(&mut output, self)?;
        encode_nodes(&mut output, &self.nodes)?;
        encode_snapshots(&mut output, &self.snapshots)?;
        Ok(output)
    }
}

fn encode_proof_header(
    output: &mut Vec<u8>,
    artifact: &ProofArtifact,
) -> Result<(), ProofArtifactError> {
    output.extend_from_slice(MAGIC);
    put_u16(output, VERSION);
    output.push(F64_BITS_CODEC);
    put_u32(output, artifact.modulus);
    put_optional_f64(output, artifact.threshold);
    put_usize(output, artifact.nodes.len())?;
    put_usize(output, artifact.snapshots.len())?;
    Ok(())
}

fn encode_nodes(output: &mut Vec<u8>, nodes: &[ProofNode]) -> Result<(), ProofArtifactError> {
    for node in nodes {
        encode_node(output, node)?;
    }
    Ok(())
}

fn encode_node(output: &mut Vec<u8>, node: &ProofNode) -> Result<(), ProofArtifactError> {
    output.extend_from_slice(&node.digest);
    encode_node_counts(output, node)?;
    encode_usizes(output, &node.vertices)?;
    encode_edges(output, &node.edges)?;
    encode_columns(output, &node.edge_columns)?;
    encode_columns(output, &node.triangle_columns)
}

fn encode_node_counts(output: &mut Vec<u8>, node: &ProofNode) -> Result<(), ProofArtifactError> {
    put_usize(output, node.vertices.len())?;
    put_usize(output, node.edges.len())?;
    put_usize(output, node.edge_columns.len())?;
    put_usize(output, node.triangle_columns.len())?;
    Ok(())
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<(), ProofArtifactError> {
    for &value in values {
        put_usize(output, value)?;
    }
    Ok(())
}

fn encode_edges(output: &mut Vec<u8>, edges: &[[usize; 2]]) -> Result<(), ProofArtifactError> {
    for &[u, v] in edges {
        put_usize(output, u)?;
        put_usize(output, v)?;
    }
    Ok(())
}

fn encode_snapshots(
    output: &mut Vec<u8>,
    snapshots: &[ProofSnapshot],
) -> Result<(), ProofArtifactError> {
    for snapshot in snapshots {
        encode_snapshot(output, snapshot)?;
    }
    Ok(())
}

fn encode_snapshot(
    output: &mut Vec<u8>,
    snapshot: &ProofSnapshot,
) -> Result<(), ProofArtifactError> {
    encode_snapshot_counts(output, snapshot)?;
    encode_graph(output, &snapshot.graph)?;
    encode_digests(output, &snapshot.atom_refs);
    encode_diagram(output, &snapshot.diagram)
}

fn encode_snapshot_counts(
    output: &mut Vec<u8>,
    snapshot: &ProofSnapshot,
) -> Result<(), ProofArtifactError> {
    put_usize(output, snapshot.graph.len())?;
    put_usize(output, snapshot.graph.num_edges())?;
    put_usize(output, snapshot.atom_refs.len())?;
    put_usize(output, snapshot.diagram.len())?;
    Ok(())
}

fn encode_graph(
    output: &mut Vec<u8>,
    graph: &SparseDistanceMatrix,
) -> Result<(), ProofArtifactError> {
    for (u, v, value) in graph.edges() {
        put_usize(output, u)?;
        put_usize(output, v)?;
        put_u64(output, value.to_bits());
    }
    Ok(())
}

fn encode_digests(output: &mut Vec<u8>, digests: &[[u8; 32]]) {
    for digest in digests {
        output.extend_from_slice(digest);
    }
}

fn encode_diagram(output: &mut Vec<u8>, diagram: &[Bar]) -> Result<(), ProofArtifactError> {
    for bar in diagram {
        put_usize(output, bar.dim)?;
        put_u64(output, bar.birth.to_bits());
        put_u64(output, bar.death.to_bits());
    }
    Ok(())
}

struct ProofBuilder {
    modulus: u32,
    threshold: Option<f64>,
    nodes: Vec<ProofNode>,
    positions: BTreeMap<[u8; 32], usize>,
    snapshots: Vec<ProofSnapshot>,
}

impl ProofBuilder {
    fn new(modulus: u32, threshold: Option<f64>) -> Self {
        Self {
            modulus,
            threshold,
            nodes: Vec::new(),
            positions: BTreeMap::new(),
            snapshots: Vec::new(),
        }
    }

    fn push(&mut self, program: &PersistenceProgram) -> Result<(), ProofArtifactError> {
        if program.params().modulus != self.modulus
            || program.params().threshold.map(f64::to_bits) != self.threshold.map(f64::to_bits)
        {
            return Err(ProofArtifactError::new(
                "all proof snapshots must share a field and threshold",
            ));
        }
        let mut atom_refs = Vec::with_capacity(program.states().len());
        for state in program.states() {
            let node = ProofNode::from_state(state);
            let digest = node.digest;
            match self.positions.get(&digest).copied() {
                Some(position) => {
                    if self.nodes[position].vertices != node.vertices
                        || self.nodes[position].edges != node.edges
                        || self.nodes[position].edge_columns != node.edge_columns
                        || self.nodes[position].triangle_columns != node.triangle_columns
                    {
                        return Err(ProofArtifactError::new("reduction-node digest collision"));
                    }
                }
                None => {
                    self.positions.insert(digest, self.nodes.len());
                    self.nodes.push(node);
                }
            }
            atom_refs.push(digest);
        }
        self.snapshots.push(ProofSnapshot {
            graph: program.current_graph().clone(),
            atom_refs,
            diagram: program.result().diagram.bars.clone(),
        });
        Ok(())
    }

    fn finish(self) -> ProofArtifact {
        ProofArtifact {
            modulus: self.modulus,
            threshold: self.threshold,
            nodes: self.nodes,
            snapshots: self.snapshots,
        }
    }
}

fn digest_columns(hash: &mut Sha256, columns: &[ChangeColumn]) {
    hash.update((columns.len() as u64).to_be_bytes());
    for column in columns {
        hash.update((column.terms.len() as u64).to_be_bytes());
        for term in &column.terms {
            hash.update((term.index as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
}

fn encode_columns(
    output: &mut Vec<u8>,
    columns: &[ChangeColumn],
) -> Result<(), ProofArtifactError> {
    for column in columns {
        put_usize(output, column.terms.len())?;
        for term in &column.terms {
            put_usize(output, term.index)?;
            put_u32(output, term.coefficient);
        }
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

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), ProofArtifactError> {
    let value = u64::try_from(value)
        .map_err(|_| ProofArtifactError::new("integer does not fit the proof format"))?;
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
    use crate::ProgramUpdateMode;

    fn graph(offset: f64) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0 + offset),
                (1, 2, 2.0 + offset),
                (2, 3, 3.0 + offset),
                (0, 3, 4.0 + offset),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, 4.5),
            ],
        )
        .unwrap()
    }

    #[test]
    fn proof_dag_reuses_unchanged_atom_nodes() {
        let initial = graph(0.0);
        let updated = graph(0.01);
        let artifact = ProofArtifact::build(
            &initial,
            &[updated],
            &RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        let summary = artifact.summary();
        assert_eq!(summary.snapshots, 2);
        assert_eq!(summary.node_references, 4);
        assert!(summary.unique_nodes < summary.node_references);
        assert!(summary.reused_references >= 1);
        assert!(artifact.encode().unwrap().starts_with(MAGIC));

        let mut program = PersistenceProgram::compile(
            &initial,
            &RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(
            program.advance(&graph(0.02)).unwrap().mode,
            ProgramUpdateMode::Reused
        );
        ProofArtifact::from_program(&program).unwrap();
    }
}
