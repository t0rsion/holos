use std::fmt;

use sha2::{Digest, Sha256};

use crate::{F64_BITS_CODEC, MAGIC, VERSION};

use super::graph::{check_columns, check_diagram, checked_threshold};
use super::verify::{
    check_bundle_nodes, check_bundle_snapshots, check_modulus, check_node_edges, check_node_scope,
};
use super::wire::{
    DecodeTotals, Reader, decode_bundle_prefix, decode_nodes, decode_snapshots, digest_columns,
    encode_node, encode_snapshot, put_optional_f64, put_u16, put_u32, put_usize,
};

/// Failure while decoding or checking a proof artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofError {
    message: String,
}

impl ProofError {
    /// Construct an error for an external proof-object source.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated proof rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProofError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "holos proof: {}", self.message)
    }
}

impl std::error::Error for ProofError {}

/// Limits enforced before proof collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProofLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted unique reduction-node count.
    pub max_nodes: usize,
    /// Largest accepted snapshot count.
    pub max_snapshots: usize,
    /// Largest accepted vertex count in one graph or node.
    pub max_vertices: usize,
    /// Largest accepted total edge count.
    pub max_edges: usize,
    /// Largest accepted total triangle count.
    pub max_triangles: usize,
    /// Largest accepted total simplex count above dimension two.
    pub max_higher_simplices: usize,
    /// Largest homology dimension accepted in a graded index proof.
    pub max_dimension: usize,
    /// Largest accepted total change-of-basis term count.
    pub max_terms: usize,
    /// Largest accepted total node-reference count.
    pub max_references: usize,
    /// Largest accepted total diagram bar count.
    pub max_bars: usize,
}

impl Default for ProofLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_nodes: 50_000_000,
            max_snapshots: 10_000_000,
            max_vertices: 1_000_000,
            max_edges: 200_000_000,
            max_triangles: 200_000_000,
            max_higher_simplices: 200_000_000,
            max_dimension: 8,
            max_terms: 400_000_000,
            max_references: 200_000_000,
            max_bars: 200_000_000,
        }
    }
}

/// One undirected weighted edge in canonical endpoint order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProofEdge {
    /// Lower endpoint.
    pub u: usize,
    /// Higher endpoint.
    pub v: usize,
    /// Non-negative finite edge weight.
    pub value: f64,
}

/// One persistence interval carried by a checked snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProofBar {
    /// Homology dimension.
    pub dimension: usize,
    /// Birth value.
    pub birth: f64,
    /// Death value. An essential class has death = f64::INFINITY.
    pub death: f64,
}

/// One nonzero change-of-basis coefficient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProofTerm {
    /// Source-column position.
    pub index: usize,
    /// Coefficient in the declared prime field.
    pub coefficient: u32,
}

/// One unit-triangular change-of-basis column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProofColumn {
    /// Nonzero terms in ascending source position.
    pub terms: Vec<ProofTerm>,
}

/// One content-addressed local reduction node.
#[derive(Debug, Clone)]
pub struct AtomProof {
    pub(crate) digest: [u8; 32],
    pub(crate) vertices: Vec<usize>,
    pub(crate) edges: Vec<[usize; 2]>,
    pub(crate) edge_columns: Vec<ProofColumn>,
    pub(crate) triangle_columns: Vec<ProofColumn>,
}

impl AtomProof {
    /// Construct a reduction node and derive its content digest.
    pub fn new(
        vertices: Vec<usize>,
        edges: Vec<[usize; 2]>,
        edge_columns: Vec<ProofColumn>,
        triangle_columns: Vec<ProofColumn>,
    ) -> Result<Self, ProofError> {
        let mut proof = Self {
            digest: [0; 32],
            vertices,
            edges,
            edge_columns,
            triangle_columns,
        };
        proof.check_shape(u32::MAX)?;
        proof.digest = proof.compute_digest();
        Ok(proof)
    }

    /// Content digest used by snapshot references.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }

    /// Original labeled vertices in this node.
    pub fn vertices(&self) -> &[usize] {
        &self.vertices
    }

    /// Original labeled terminal edges in this node.
    pub fn edges(&self) -> &[[usize; 2]] {
        &self.edges
    }

    /// Edge-boundary change columns.
    pub fn edge_columns(&self) -> &[ProofColumn] {
        &self.edge_columns
    }

    /// Triangle-boundary change columns.
    pub fn triangle_columns(&self) -> &[ProofColumn] {
        &self.triangle_columns
    }

    pub(crate) fn compute_digest(&self) -> [u8; 32] {
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

    pub(crate) fn check_shape(&self, modulus: u32) -> Result<(), ProofError> {
        check_node_scope(&self.vertices, &self.edges)?;
        check_node_edges(&self.vertices, &self.edges)?;
        if modulus != u32::MAX {
            check_columns(&self.edge_columns, modulus)?;
            check_columns(&self.triangle_columns, modulus)?;
        }
        Ok(())
    }
}

/// One graph state and its references into the reduction-node table.
#[derive(Debug, Clone)]
pub struct SnapshotProof {
    pub(crate) vertex_count: usize,
    pub(crate) edges: Vec<ProofEdge>,
    pub(crate) atom_refs: Vec<[u8; 32]>,
    pub(crate) diagram: Vec<ProofBar>,
}

impl SnapshotProof {
    /// Construct one snapshot record.
    pub fn new(
        vertex_count: usize,
        edges: Vec<ProofEdge>,
        atom_refs: Vec<[u8; 32]>,
        diagram: Vec<ProofBar>,
    ) -> Result<Self, ProofError> {
        let snapshot = Self {
            vertex_count,
            edges,
            atom_refs,
            diagram,
        };
        snapshot.check_shape()?;
        Ok(snapshot)
    }

    /// Number of labeled vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }

    /// Complete listed graph.
    pub fn edges(&self) -> &[ProofEdge] {
        &self.edges
    }

    /// Ordered reduction-node digests for cyclic atoms.
    pub fn atom_refs(&self) -> &[[u8; 32]] {
        &self.atom_refs
    }

    /// Declared exact H0 and H1 diagram.
    pub fn diagram(&self) -> &[ProofBar] {
        &self.diagram
    }

    pub(crate) fn check_shape(&self) -> Result<(), ProofError> {
        if self.vertex_count == 0 {
            return Err(ProofError::new("snapshot has no vertices"));
        }
        let mut previous = None;
        for edge in &self.edges {
            if edge.u >= edge.v
                || edge.v >= self.vertex_count
                || !edge.value.is_finite()
                || edge.value < 0.0
                || previous.is_some_and(|value| value >= (edge.u, edge.v))
            {
                return Err(ProofError::new("snapshot graph is not canonical"));
            }
            previous = Some((edge.u, edge.v));
        }
        check_diagram(&self.diagram)?;
        Ok(())
    }
}

/// Bounded content-addressed `HOLOSPF` persistence proof.
#[derive(Debug, Clone)]
pub struct ProofBundle {
    pub(crate) modulus: u32,
    pub(crate) threshold: Option<f64>,
    pub(crate) nodes: Vec<AtomProof>,
    pub(crate) snapshots: Vec<SnapshotProof>,
}

impl ProofBundle {
    /// Construct a proof bundle from unique nodes and ordered snapshots.
    pub fn new(
        modulus: u32,
        threshold: Option<f64>,
        nodes: Vec<AtomProof>,
        snapshots: Vec<SnapshotProof>,
    ) -> Result<Self, ProofError> {
        let proof = Self {
            modulus,
            threshold,
            nodes,
            snapshots,
        };
        proof.check_shape()?;
        Ok(proof)
    }

    /// Prime coefficient modulus.
    pub fn modulus(&self) -> u32 {
        self.modulus
    }

    /// Fixed filtration threshold.
    pub fn threshold(&self) -> Option<f64> {
        self.threshold
    }

    /// Unique content-addressed reduction nodes.
    pub fn nodes(&self) -> &[AtomProof] {
        &self.nodes
    }

    /// Ordered graph states in the trajectory.
    pub fn snapshots(&self) -> &[SnapshotProof] {
        &self.snapshots
    }

    /// Encode the canonical `HOLOSPF` version 1 envelope.
    pub fn encode(&self) -> Result<Vec<u8>, ProofError> {
        self.check_shape()?;
        let mut output = Vec::new();
        output.extend_from_slice(MAGIC);
        put_u16(&mut output, VERSION);
        output.push(F64_BITS_CODEC);
        put_u32(&mut output, self.modulus);
        put_optional_f64(&mut output, self.threshold);
        put_usize(&mut output, self.nodes.len())?;
        put_usize(&mut output, self.snapshots.len())?;
        for node in &self.nodes {
            encode_node(&mut output, node)?;
        }
        for snapshot in &self.snapshots {
            encode_snapshot(&mut output, snapshot)?;
        }
        Ok(output)
    }

    /// Decode and structurally validate a bounded proof envelope.
    pub fn decode(bytes: &[u8], limits: ProofLimits) -> Result<Self, ProofError> {
        if bytes.len() > limits.max_bytes {
            return Err(ProofError::new(format!(
                "{} bytes exceed the limit {}",
                bytes.len(),
                limits.max_bytes
            )));
        }
        let mut reader = Reader::new(bytes);
        decode_bundle_prefix(&mut reader)?;
        let modulus = reader.u32()?;
        let threshold = reader.optional_f64()?;
        let node_count = reader.bounded_usize("node count", limits.max_nodes)?;
        let snapshot_count = reader.bounded_usize("snapshot count", limits.max_snapshots)?;
        let mut totals = DecodeTotals::default();
        let nodes = decode_nodes(&mut reader, node_count, limits, modulus, &mut totals)?;
        let snapshots = decode_snapshots(&mut reader, snapshot_count, limits, &mut totals)?;
        if reader.remaining() != 0 {
            return Err(ProofError::new(format!(
                "{} trailing bytes after the envelope",
                reader.remaining()
            )));
        }
        Self::new(modulus, threshold, nodes, snapshots)
    }

    pub(crate) fn check_shape(&self) -> Result<(), ProofError> {
        check_modulus(self.modulus)?;
        checked_threshold(self.threshold)?;
        if self.snapshots.is_empty() {
            return Err(ProofError::new("proof has no snapshots"));
        }
        let digests = check_bundle_nodes(&self.nodes, self.modulus)?;
        check_bundle_snapshots(&self.snapshots, &digests)?;
        Ok(())
    }
}
