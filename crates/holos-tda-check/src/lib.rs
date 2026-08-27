#![forbid(unsafe_code)]
#![warn(missing_docs)]

//! Solver-independent verification of proof-carrying sparse persistence.
//!
//! The crate has no dependency on `holos-tda`. It checks bounded `HOLOSPF`
//! envelopes by reconstructing filtered edge and triangle boundaries,
//! checking every declared `D V = R` factorization, checking the structural
//! decomposition, and composing exact H0 and H1 diagrams.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use sha2::{Digest, Sha256};

mod cohomology;
mod cohomology_intervention;
mod coverage;
mod coverage_geometry;
mod distributed;
mod explicit;
mod index;
mod kinetic_zigzag;
mod relative;
mod synthesis;

pub use cohomology_intervention::{
    VerifiedCohomologyIntervention, VerifiedCohomologyInterventionStatus,
    is_cohomology_intervention, verify_cohomology_intervention,
};
pub use coverage::{
    VerifiedCoverage, VerifiedCoverageSource, VerifiedCoverageStatus, is_coverage, verify_coverage,
};
pub use coverage_geometry::{
    VerifiedGeometryBoundCoverage, is_geometry_bound_coverage, verify_geometry_bound_coverage,
};
pub use distributed::{
    VerifiedDistributedInterface, is_distributed_interface, verify_distributed_interface,
    verify_distributed_interface_with,
};
pub use explicit::{
    VerifiedExplicitPersistence, is_explicit_persistence, verify_explicit_persistence,
};
pub use index::{IndexProofState, VerifiedIndexDelta, VerifiedIndexSnapshot, is_index_snapshot};
pub use kinetic_zigzag::{VerifiedKineticZigzag, is_kinetic_zigzag, verify_kinetic_zigzag};
pub use relative::{
    VerifiedRelativeInterface, is_relative_interface, verify_relative_composition,
    verify_relative_interface,
};
pub use synthesis::{
    VerifiedSynthesis, VerifiedSynthesisSource, VerifiedSynthesisStatus, is_synthesis,
    verify_synthesis,
};

const MAGIC: &[u8; 8] = b"HOLOSPF\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u64 = 32_768;
const SEPARATOR_WIDTH: usize = 3;
const SEPARATOR_SEARCH_LIMIT: usize = 100_000;

/// Failure while decoding or checking a proof bundle.
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
    /// Death value, or positive infinity for an essential interval.
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
    digest: [u8; 32],
    vertices: Vec<usize>,
    edges: Vec<[usize; 2]>,
    edge_columns: Vec<ProofColumn>,
    triangle_columns: Vec<ProofColumn>,
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

    fn check_shape(&self, modulus: u32) -> Result<(), ProofError> {
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
    vertex_count: usize,
    edges: Vec<ProofEdge>,
    atom_refs: Vec<[u8; 32]>,
    diagram: Vec<ProofBar>,
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

    fn check_shape(&self) -> Result<(), ProofError> {
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

/// A bounded content-addressed persistence proof and delta trajectory.
#[derive(Debug, Clone)]
pub struct ProofBundle {
    modulus: u32,
    threshold: Option<f64>,
    nodes: Vec<AtomProof>,
    snapshots: Vec<SnapshotProof>,
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

    fn check_shape(&self) -> Result<(), ProofError> {
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

fn check_node_scope(vertices: &[usize], edges: &[[usize; 2]]) -> Result<(), ProofError> {
    if vertices.len() < 3
        || vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || edges.len() < vertices.len()
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        Err(ProofError::new("reduction node scope is not canonical"))
    } else {
        Ok(())
    }
}

fn check_node_edges(vertices: &[usize], edges: &[[usize; 2]]) -> Result<(), ProofError> {
    if edges.iter().any(|&[u, v]| {
        u >= v || vertices.binary_search(&u).is_err() || vertices.binary_search(&v).is_err()
    }) {
        Err(ProofError::new("reduction node has an invalid edge"))
    } else {
        Ok(())
    }
}

fn check_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        Err(ProofError::new(format!(
            "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
        )))
    } else {
        Ok(())
    }
}

fn check_bundle_nodes(nodes: &[AtomProof], modulus: u32) -> Result<BTreeSet<[u8; 32]>, ProofError> {
    let mut digests = BTreeSet::new();
    for node in nodes {
        node.check_shape(modulus)?;
        if node.compute_digest() != node.digest || !digests.insert(node.digest) {
            return Err(ProofError::new(
                "reduction-node digest is wrong or duplicated",
            ));
        }
    }
    Ok(digests)
}

fn check_bundle_snapshots(
    snapshots: &[SnapshotProof],
    digests: &BTreeSet<[u8; 32]>,
) -> Result<(), ProofError> {
    for snapshot in snapshots {
        snapshot.check_shape()?;
        if snapshot
            .atom_refs
            .iter()
            .any(|digest| !digests.contains(digest))
        {
            return Err(ProofError::new(
                "snapshot references an unknown reduction node",
            ));
        }
    }
    Ok(())
}

/// Counts derived after complete solver-independent verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedProof {
    /// Checked snapshot count.
    pub snapshots: usize,
    /// Unique checked reduction-node count.
    pub unique_nodes: usize,
    /// Snapshot references already present in the previous snapshot.
    pub reused_references: usize,
    /// Node references whose identical weighted reduction was already checked.
    pub cached_references: usize,
    /// Edge-boundary columns checked across all node references.
    pub edge_columns_checked: usize,
    /// Triangle-boundary columns checked across all node references.
    pub triangle_columns_checked: usize,
}

impl ProofBundle {
    /// Verify every snapshot without calling a persistence solver.
    pub fn verify(&self) -> Result<VerifiedProof, ProofError> {
        self.check_shape()?;
        let mut verifier = BundleVerifier::new(self);
        for (snapshot_index, snapshot) in self.snapshots.iter().enumerate() {
            verifier.verify_snapshot(snapshot_index, snapshot)?;
        }
        Ok(verifier.summary())
    }
}

type ReductionCache = BTreeMap<([u8; 32], Vec<u64>), Vec<ProofBar>>;

struct BundleVerifier<'a> {
    bundle: &'a ProofBundle,
    nodes: BTreeMap<[u8; 32], &'a AtomProof>,
    previous: BTreeSet<[u8; 32]>,
    reductions: ReductionCache,
    reused_references: usize,
    cached_references: usize,
    edge_columns_checked: usize,
    triangle_columns_checked: usize,
}

impl<'a> BundleVerifier<'a> {
    fn new(bundle: &'a ProofBundle) -> Self {
        Self {
            bundle,
            nodes: bundle
                .nodes
                .iter()
                .map(|node| (node.digest, node))
                .collect(),
            previous: BTreeSet::new(),
            reductions: BTreeMap::new(),
            reused_references: 0,
            cached_references: 0,
            edge_columns_checked: 0,
            triangle_columns_checked: 0,
        }
    }

    fn verify_snapshot(
        &mut self,
        snapshot_index: usize,
        snapshot: &SnapshotProof,
    ) -> Result<(), ProofError> {
        let graph = Graph::new(snapshot.vertex_count, &snapshot.edges)?;
        let cyclic = program_blocks(&graph, self.bundle.threshold)?
            .into_iter()
            .filter(|block| block.edges.len() >= block.vertices.len())
            .collect::<Vec<_>>();
        verify_atom_count(snapshot_index, snapshot, cyclic.len())?;
        let mut diagram = h0_diagram(&graph, self.bundle.threshold)?;
        for (position, (block, digest)) in cyclic.iter().zip(&snapshot.atom_refs).enumerate() {
            diagram.extend(self.verify_atom(snapshot_index, position, &graph, block, digest)?);
        }
        canonicalize_diagram(&mut diagram);
        verify_snapshot_diagram(snapshot_index, &diagram, &snapshot.diagram)?;
        self.record_reuse(&snapshot.atom_refs);
        Ok(())
    }

    fn verify_atom(
        &mut self,
        snapshot: usize,
        position: usize,
        graph: &Graph,
        block: &Block,
        digest: &[u8; 32],
    ) -> Result<Vec<ProofBar>, ProofError> {
        let node = self.nodes[digest];
        if node.vertices != block.vertices || node.edges != block.edges {
            return Err(ProofError::new(format!(
                "snapshot {snapshot} atom {position} differs from the checked decomposition"
            )));
        }
        let local = graph.local(&node.vertices, &node.edges)?;
        self.h1_for_node(node, &local)
    }

    fn h1_for_node(
        &mut self,
        node: &AtomProof,
        local: &Graph,
    ) -> Result<Vec<ProofBar>, ProofError> {
        let key = (
            node.digest,
            local
                .edges
                .iter()
                .map(|edge| edge.value.to_bits())
                .collect(),
        );
        if let Some(checked) = self.reductions.get(&key) {
            self.cached_references += 1;
            return Ok(checked.clone());
        }
        let checked = check_reduction(
            local,
            self.bundle.threshold,
            self.bundle.modulus,
            &node.edge_columns,
            &node.triangle_columns,
        )?;
        self.edge_columns_checked += node.edge_columns.len();
        self.triangle_columns_checked += node.triangle_columns.len();
        let h1 = checked
            .diagram
            .into_iter()
            .filter(|bar| bar.dimension == 1)
            .collect::<Vec<_>>();
        self.reductions.insert(key, h1.clone());
        Ok(h1)
    }

    fn record_reuse(&mut self, references: &[[u8; 32]]) {
        let current = references.iter().copied().collect::<BTreeSet<_>>();
        self.reused_references += current.intersection(&self.previous).count();
        self.previous = current;
    }

    fn summary(&self) -> VerifiedProof {
        VerifiedProof {
            snapshots: self.bundle.snapshots.len(),
            unique_nodes: self.bundle.nodes.len(),
            reused_references: self.reused_references,
            cached_references: self.cached_references,
            edge_columns_checked: self.edge_columns_checked,
            triangle_columns_checked: self.triangle_columns_checked,
        }
    }
}

fn verify_atom_count(
    snapshot_index: usize,
    snapshot: &SnapshotProof,
    actual: usize,
) -> Result<(), ProofError> {
    if actual != snapshot.atom_refs.len() {
        Err(ProofError::new(format!(
            "snapshot {snapshot_index} has {actual} cyclic atoms but {} references",
            snapshot.atom_refs.len()
        )))
    } else {
        Ok(())
    }
}

fn verify_snapshot_diagram(
    snapshot_index: usize,
    checked: &[ProofBar],
    claimed: &[ProofBar],
) -> Result<(), ProofError> {
    if diagrams_equal(checked, claimed) {
        Ok(())
    } else {
        Err(ProofError::new(format!(
            "snapshot {snapshot_index} diagram differs from the checked composition"
        )))
    }
}

fn decode_bundle_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC {
        return Err(ProofError::new("wrong magic bytes"));
    }
    if reader.u16()? != VERSION {
        return Err(ProofError::new("unsupported wire version"));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported scalar codec"));
    }
    Ok(())
}

fn decode_nodes(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProofLimits,
    modulus: u32,
    totals: &mut DecodeTotals,
) -> Result<Vec<AtomProof>, ProofError> {
    (0..count)
        .map(|_| decode_node(reader, limits, modulus, totals))
        .collect()
}

fn decode_snapshots(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProofLimits,
    totals: &mut DecodeTotals,
) -> Result<Vec<SnapshotProof>, ProofError> {
    (0..count)
        .map(|_| decode_snapshot(reader, limits, totals))
        .collect()
}

fn encode_node(output: &mut Vec<u8>, node: &AtomProof) -> Result<(), ProofError> {
    output.extend_from_slice(&node.digest);
    put_usize(output, node.vertices.len())?;
    put_usize(output, node.edges.len())?;
    put_usize(output, node.edge_columns.len())?;
    put_usize(output, node.triangle_columns.len())?;
    encode_usizes(output, &node.vertices)?;
    encode_edge_keys(output, &node.edges)?;
    encode_columns(output, &node.edge_columns)?;
    encode_columns(output, &node.triangle_columns)?;
    Ok(())
}

fn encode_columns(output: &mut Vec<u8>, columns: &[ProofColumn]) -> Result<(), ProofError> {
    for column in columns {
        put_usize(output, column.terms.len())?;
        for term in &column.terms {
            put_usize(output, term.index)?;
            put_u32(output, term.coefficient);
        }
    }
    Ok(())
}

fn encode_snapshot(output: &mut Vec<u8>, snapshot: &SnapshotProof) -> Result<(), ProofError> {
    put_usize(output, snapshot.vertex_count)?;
    put_usize(output, snapshot.edges.len())?;
    put_usize(output, snapshot.atom_refs.len())?;
    put_usize(output, snapshot.diagram.len())?;
    encode_proof_edges(output, &snapshot.edges)?;
    for digest in &snapshot.atom_refs {
        output.extend_from_slice(digest);
    }
    encode_bars(output, &snapshot.diagram)?;
    Ok(())
}

fn encode_usizes(output: &mut Vec<u8>, values: &[usize]) -> Result<(), ProofError> {
    for &value in values {
        put_usize(output, value)?;
    }
    Ok(())
}

fn encode_edge_keys(output: &mut Vec<u8>, edges: &[[usize; 2]]) -> Result<(), ProofError> {
    for &[u, v] in edges {
        put_usize(output, u)?;
        put_usize(output, v)?;
    }
    Ok(())
}

fn encode_proof_edges(output: &mut Vec<u8>, edges: &[ProofEdge]) -> Result<(), ProofError> {
    for edge in edges {
        put_usize(output, edge.u)?;
        put_usize(output, edge.v)?;
        put_u64(output, edge.value.to_bits());
    }
    Ok(())
}

fn encode_bars(output: &mut Vec<u8>, bars: &[ProofBar]) -> Result<(), ProofError> {
    for bar in bars {
        put_usize(output, bar.dimension)?;
        put_u64(output, bar.birth.to_bits());
        put_u64(output, bar.death.to_bits());
    }
    Ok(())
}

#[derive(Default)]
struct DecodeTotals {
    vertices: usize,
    edges: usize,
    triangles: usize,
    terms: usize,
    references: usize,
    bars: usize,
}

struct NodeHeader {
    digest: [u8; 32],
    vertices: usize,
    edges: usize,
    edge_columns: usize,
    triangle_columns: usize,
}

struct SnapshotHeader {
    vertices: usize,
    edges: usize,
    references: usize,
    bars: usize,
}

fn decode_node(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
    modulus: u32,
    totals: &mut DecodeTotals,
) -> Result<AtomProof, ProofError> {
    let header = decode_node_header(reader, limits)?;
    record_node_totals(totals, &header, limits)?;
    let vertices = decode_usizes(reader, header.vertices)?;
    let edges = decode_edge_keys(reader, header.edges)?;
    let edge_columns = decode_columns(
        reader,
        header.edge_columns,
        modulus,
        limits.max_terms,
        &mut totals.terms,
    )?;
    let triangle_columns = decode_columns(
        reader,
        header.triangle_columns,
        modulus,
        limits.max_terms,
        &mut totals.terms,
    )?;
    finish_node(
        header.digest,
        vertices,
        edges,
        edge_columns,
        triangle_columns,
        modulus,
    )
}

fn decode_node_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<NodeHeader, ProofError> {
    Ok(NodeHeader {
        digest: reader.array32()?,
        vertices: reader.bounded_usize("node vertex count", limits.max_vertices)?,
        edges: reader.usize()?,
        edge_columns: reader.usize()?,
        triangle_columns: reader.usize()?,
    })
}

fn record_node_totals(
    totals: &mut DecodeTotals,
    header: &NodeHeader,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    totals.vertices = bounded_sum(
        totals.vertices,
        header.vertices,
        limits.max_vertices.saturating_mul(limits.max_nodes),
        "node vertices",
    )?;
    totals.edges = bounded_sum(totals.edges, header.edges, limits.max_edges, "node edges")?;
    totals.triangles = bounded_sum(
        totals.triangles,
        header.triangle_columns,
        limits.max_triangles,
        "triangle columns",
    )?;
    Ok(())
}

fn decode_usizes(reader: &mut Reader<'_>, count: usize) -> Result<Vec<usize>, ProofError> {
    (0..count).map(|_| reader.usize()).collect()
}

fn decode_edge_keys(reader: &mut Reader<'_>, count: usize) -> Result<Vec<[usize; 2]>, ProofError> {
    (0..count)
        .map(|_| Ok([reader.usize()?, reader.usize()?]))
        .collect()
}

fn finish_node(
    digest: [u8; 32],
    vertices: Vec<usize>,
    edges: Vec<[usize; 2]>,
    edge_columns: Vec<ProofColumn>,
    triangle_columns: Vec<ProofColumn>,
    modulus: u32,
) -> Result<AtomProof, ProofError> {
    let node = AtomProof {
        digest,
        vertices,
        edges,
        edge_columns,
        triangle_columns,
    };
    node.check_shape(modulus)?;
    if node.compute_digest() != digest {
        return Err(ProofError::new("reduction-node digest does not match"));
    }
    Ok(node)
}

fn decode_columns(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
) -> Result<Vec<ProofColumn>, ProofError> {
    let mut columns = Vec::with_capacity(count);
    for target in 0..count {
        let term_count = reader.usize()?;
        *total_terms = bounded_sum(*total_terms, term_count, term_limit, "change terms")?;
        let mut terms = Vec::with_capacity(term_count);
        for _ in 0..term_count {
            terms.push(ProofTerm {
                index: reader.usize()?,
                coefficient: reader.u32()?,
            });
        }
        check_column(target, &terms, modulus)?;
        columns.push(ProofColumn { terms });
    }
    Ok(columns)
}

fn decode_snapshot(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
    totals: &mut DecodeTotals,
) -> Result<SnapshotProof, ProofError> {
    let header = decode_snapshot_header(reader, limits)?;
    record_snapshot_totals(totals, &header, limits)?;
    let edges = decode_proof_edges(reader, header.edges)?;
    let atom_refs = (0..header.references)
        .map(|_| reader.array32())
        .collect::<Result<Vec<_>, _>>()?;
    let diagram = decode_bars(reader, header.bars)?;
    SnapshotProof::new(header.vertices, edges, atom_refs, diagram)
}

fn decode_snapshot_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<SnapshotHeader, ProofError> {
    Ok(SnapshotHeader {
        vertices: reader.bounded_usize("snapshot vertex count", limits.max_vertices)?,
        edges: reader.usize()?,
        references: reader.usize()?,
        bars: reader.usize()?,
    })
}

fn record_snapshot_totals(
    totals: &mut DecodeTotals,
    header: &SnapshotHeader,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    totals.edges = bounded_sum(
        totals.edges,
        header.edges,
        limits.max_edges,
        "snapshot edges",
    )?;
    totals.references = bounded_sum(
        totals.references,
        header.references,
        limits.max_references,
        "node references",
    )?;
    totals.bars = bounded_sum(totals.bars, header.bars, limits.max_bars, "diagram bars")?;
    Ok(())
}

fn decode_proof_edges(reader: &mut Reader<'_>, count: usize) -> Result<Vec<ProofEdge>, ProofError> {
    (0..count)
        .map(|_| {
            Ok(ProofEdge {
                u: reader.usize()?,
                v: reader.usize()?,
                value: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
}

fn decode_bars(reader: &mut Reader<'_>, count: usize) -> Result<Vec<ProofBar>, ProofError> {
    (0..count)
        .map(|_| {
            Ok(ProofBar {
                dimension: reader.usize()?,
                birth: f64::from_bits(reader.u64()?),
                death: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
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

#[derive(Debug, Clone)]
struct Graph {
    vertex_count: usize,
    edges: Vec<ProofEdge>,
    positions: BTreeMap<(usize, usize), usize>,
}

impl Graph {
    fn new(vertex_count: usize, edges: &[ProofEdge]) -> Result<Self, ProofError> {
        let positions: BTreeMap<_, _> = edges
            .iter()
            .enumerate()
            .map(|(position, edge)| ((edge.u, edge.v), position))
            .collect();
        if positions.len() != edges.len() {
            return Err(ProofError::new("graph contains duplicate edges"));
        }
        Ok(Self {
            vertex_count,
            edges: edges.to_vec(),
            positions,
        })
    }

    fn get(&self, u: usize, v: usize) -> f64 {
        let edge = if u < v { (u, v) } else { (v, u) };
        self.positions
            .get(&edge)
            .map_or(f64::INFINITY, |&position| self.edges[position].value)
    }

    fn local(&self, vertices: &[usize], edges: &[[usize; 2]]) -> Result<Self, ProofError> {
        let local_positions: BTreeMap<_, _> = vertices
            .iter()
            .copied()
            .enumerate()
            .map(|(position, vertex)| (vertex, position))
            .collect();
        let mut local_edges = Vec::with_capacity(edges.len());
        for &[u, v] in edges {
            local_edges.push(ProofEdge {
                u: local_positions[&u],
                v: local_positions[&v],
                value: self.get(u, v),
            });
        }
        local_edges.sort_by_key(|edge| (edge.u, edge.v));
        Self::new(vertices.len(), &local_edges)
    }
}

#[derive(Debug, Clone)]
struct Block {
    vertices: Vec<usize>,
    edges: Vec<[usize; 2]>,
}

fn program_blocks(graph: &Graph, threshold: Option<f64>) -> Result<Vec<Block>, ProofError> {
    let threshold = checked_threshold(threshold)?;
    let active: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .copied()
        .collect();
    let mut adjacency = vec![Vec::<(usize, usize)>::new(); graph.vertex_count];
    for (position, edge) in active.iter().enumerate() {
        adjacency[edge.u].push((edge.v, position));
        adjacency[edge.v].push((edge.u, position));
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable();
    }
    let edge_blocks = biconnected_edge_blocks(&adjacency);
    let mut initial = Vec::new();
    for positions in edge_blocks {
        let mut vertices = Vec::new();
        let mut edges = Vec::new();
        for position in positions {
            let edge = active[position];
            vertices.push(edge.u);
            vertices.push(edge.v);
            edges.push([edge.u, edge.v]);
        }
        vertices.sort_unstable();
        vertices.dedup();
        edges.sort_unstable();
        initial.push(Block { vertices, edges });
    }
    let mut search = SeparatorSearch {
        graph,
        checked: 0,
        complete: true,
    };
    let mut blocks = Vec::new();
    for block in initial {
        search.refine(block, &mut blocks);
    }
    blocks.sort_by(|left, right| {
        left.vertices
            .cmp(&right.vertices)
            .then(left.edges.cmp(&right.edges))
    });
    Ok(blocks)
}

fn biconnected_edge_blocks(adjacency: &[Vec<(usize, usize)>]) -> Vec<Vec<usize>> {
    BiconnectedSearch::new(adjacency).run()
}

struct BiconnectedSearch<'a> {
    adjacency: &'a [Vec<(usize, usize)>],
    discovered: Vec<usize>,
    low: Vec<usize>,
    next: Vec<usize>,
    parent_edge: Vec<usize>,
    time: usize,
    path: Vec<usize>,
    edge_stack: Vec<usize>,
    blocks: Vec<Vec<usize>>,
}

impl<'a> BiconnectedSearch<'a> {
    fn new(adjacency: &'a [Vec<(usize, usize)>]) -> Self {
        let vertices = adjacency.len();
        Self {
            adjacency,
            discovered: vec![usize::MAX; vertices],
            low: vec![0; vertices],
            next: vec![0; vertices],
            parent_edge: vec![usize::MAX; vertices],
            time: 0,
            path: Vec::new(),
            edge_stack: Vec::new(),
            blocks: Vec::new(),
        }
    }

    fn run(mut self) -> Vec<Vec<usize>> {
        for root in 0..self.adjacency.len() {
            if self.discovered[root] == usize::MAX && !self.adjacency[root].is_empty() {
                self.start_root(root);
                self.walk();
            }
        }
        for block in &mut self.blocks {
            block.sort_unstable();
        }
        self.blocks.sort_unstable();
        self.blocks
    }

    fn start_root(&mut self, root: usize) {
        self.discovered[root] = self.time;
        self.low[root] = self.time;
        self.time += 1;
        self.path.push(root);
    }

    fn walk(&mut self) {
        while let Some(&vertex) = self.path.last() {
            if self.next[vertex] < self.adjacency[vertex].len() {
                let (neighbor, edge) = self.adjacency[vertex][self.next[vertex]];
                self.next[vertex] += 1;
                self.visit_edge(vertex, neighbor, edge);
            } else {
                self.path.pop();
                self.finish_vertex(vertex);
            }
        }
    }

    fn visit_edge(&mut self, vertex: usize, neighbor: usize, edge: usize) {
        if self.discovered[neighbor] == usize::MAX {
            self.parent_edge[neighbor] = edge;
            self.edge_stack.push(edge);
            self.discovered[neighbor] = self.time;
            self.low[neighbor] = self.time;
            self.time += 1;
            self.path.push(neighbor);
        } else if edge != self.parent_edge[vertex]
            && self.discovered[neighbor] < self.discovered[vertex]
        {
            self.low[vertex] = self.low[vertex].min(self.discovered[neighbor]);
            self.edge_stack.push(edge);
        }
    }

    fn finish_vertex(&mut self, vertex: usize) {
        let edge = self.parent_edge[vertex];
        if edge == usize::MAX {
            if !self.edge_stack.is_empty() {
                self.blocks.push(std::mem::take(&mut self.edge_stack));
            }
            return;
        }
        let parent = self.adjacency[vertex]
            .iter()
            .find_map(|&(neighbor, candidate)| (candidate == edge).then_some(neighbor))
            .expect("a tree edge has its parent endpoint");
        self.low[parent] = self.low[parent].min(self.low[vertex]);
        if self.low[vertex] >= self.discovered[parent] {
            let block = self.pop_block(edge);
            self.blocks.push(block);
        }
    }

    fn pop_block(&mut self, terminal: usize) -> Vec<usize> {
        let mut block = Vec::new();
        while let Some(candidate) = self.edge_stack.pop() {
            block.push(candidate);
            if candidate == terminal {
                break;
            }
        }
        block
    }
}

struct SeparatorSearch<'a> {
    graph: &'a Graph,
    checked: usize,
    complete: bool,
}

impl SeparatorSearch<'_> {
    fn refine(&mut self, block: Block, output: &mut Vec<Block>) {
        if !self.complete || block.edges.len() < block.vertices.len() {
            output.push(block);
            return;
        }
        let Some((separator, components)) = self.find(&block) else {
            output.push(block);
            return;
        };
        for component in components {
            let mut vertices = separator.clone();
            vertices.extend(component);
            vertices.sort_unstable();
            let members: BTreeSet<_> = vertices.iter().copied().collect();
            let edges = block
                .edges
                .iter()
                .copied()
                .filter(|[u, v]| members.contains(u) && members.contains(v))
                .collect();
            self.refine(Block { vertices, edges }, output);
        }
    }

    fn find(&mut self, block: &Block) -> Option<(Vec<usize>, Vec<Vec<usize>>)> {
        let maximum = SEPARATOR_WIDTH.min(block.vertices.len().saturating_sub(2));
        for width in 2..=maximum {
            let mut positions: Vec<_> = (0..width).collect();
            loop {
                if self.checked == SEPARATOR_SEARCH_LIMIT {
                    self.complete = false;
                    return None;
                }
                self.checked += 1;
                let separator: Vec<_> = positions
                    .iter()
                    .map(|&position| block.vertices[position])
                    .collect();
                if zero_simplex(self.graph, &separator) {
                    let components = components_without(block, &separator);
                    if components.len() > 1 {
                        return Some((separator, components));
                    }
                }
                if !next_combination(&mut positions, block.vertices.len()) {
                    break;
                }
            }
        }
        None
    }
}

fn zero_simplex(graph: &Graph, vertices: &[usize]) -> bool {
    for (position, &u) in vertices.iter().enumerate() {
        for &v in &vertices[position + 1..] {
            if graph.get(u, v).to_bits() != 0 {
                return false;
            }
        }
    }
    true
}

fn components_without(block: &Block, separator: &[usize]) -> Vec<Vec<usize>> {
    let excluded: BTreeSet<_> = separator.iter().copied().collect();
    let positions: BTreeMap<_, _> = block
        .vertices
        .iter()
        .copied()
        .enumerate()
        .map(|(position, vertex)| (vertex, position))
        .collect();
    let mut adjacency = vec![Vec::new(); block.vertices.len()];
    for &[u, v] in &block.edges {
        if excluded.contains(&u) || excluded.contains(&v) {
            continue;
        }
        adjacency[positions[&u]].push(v);
        adjacency[positions[&v]].push(u);
    }
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for &root in &block.vertices {
        if excluded.contains(&root) || !seen.insert(root) {
            continue;
        }
        let mut stack = vec![root];
        let mut component = Vec::new();
        while let Some(vertex) = stack.pop() {
            component.push(vertex);
            for &neighbor in &adjacency[positions[&vertex]] {
                if seen.insert(neighbor) {
                    stack.push(neighbor);
                }
            }
        }
        component.sort_unstable();
        components.push(component);
    }
    components.sort_unstable();
    components
}

fn next_combination(positions: &mut [usize], universe: usize) -> bool {
    for index in (0..positions.len()).rev() {
        let maximum = universe - (positions.len() - index);
        if positions[index] < maximum {
            positions[index] += 1;
            for next in index + 1..positions.len() {
                positions[next] = positions[next - 1] + 1;
            }
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
struct FilteredEdge {
    vertices: [usize; 2],
    value: f64,
}

#[derive(Clone, Copy)]
struct FilteredTriangle {
    vertices: [usize; 3],
    value: f64,
}

struct FilteredComplex {
    vertex_count: usize,
    edges: Vec<FilteredEdge>,
    triangles: Vec<FilteredTriangle>,
    edge_rows: BTreeMap<(usize, usize), usize>,
}

impl FilteredComplex {
    fn build(graph: &Graph, threshold: Option<f64>) -> Result<Self, ProofError> {
        let threshold = checked_threshold(threshold)?;
        let edges = filtered_edges(graph, threshold);
        let edge_rows = edges
            .iter()
            .enumerate()
            .map(|(position, edge)| ((edge.vertices[0], edge.vertices[1]), position))
            .collect();
        let upper = upper_neighbors(graph.vertex_count, &edges);
        let triangles = filtered_triangles(graph, &upper);
        Ok(Self {
            vertex_count: graph.vertex_count,
            edges,
            triangles,
            edge_rows,
        })
    }

    fn edge_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
        let modulus = modulus as u64;
        self.edges
            .iter()
            .map(|edge| {
                let mut column = SparseColumn::default();
                column.insert(edge.vertices[0], modulus - 1);
                column.insert(edge.vertices[1], 1);
                column
            })
            .collect()
    }

    fn triangle_boundaries(&self, modulus: u32) -> Vec<SparseColumn> {
        let modulus = modulus as u64;
        self.triangles
            .iter()
            .map(|triangle| {
                let [u, v, w] = triangle.vertices;
                let mut column = SparseColumn::default();
                column.insert(self.edge_rows[&(v, w)], 1);
                column.insert(self.edge_rows[&(u, w)], modulus - 1);
                column.insert(self.edge_rows[&(u, v)], 1);
                column
            })
            .collect()
    }
}

fn filtered_edges(graph: &Graph, threshold: f64) -> Vec<FilteredEdge> {
    let mut edges = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .map(|edge| FilteredEdge {
            vertices: [edge.u, edge.v],
            value: edge.value,
        })
        .collect::<Vec<_>>();
    edges.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| edge_rank(right.vertices).cmp(&edge_rank(left.vertices)))
    });
    edges
}

fn upper_neighbors(vertex_count: usize, edges: &[FilteredEdge]) -> Vec<Vec<usize>> {
    let mut upper = vec![Vec::new(); vertex_count];
    for edge in edges {
        upper[edge.vertices[0]].push(edge.vertices[1]);
    }
    for neighbors in &mut upper {
        neighbors.sort_unstable();
    }
    upper
}

fn filtered_triangles(graph: &Graph, upper: &[Vec<usize>]) -> Vec<FilteredTriangle> {
    let mut triangles = Vec::new();
    for u in 0..graph.vertex_count {
        for &v in &upper[u] {
            append_common_triangles(&mut triangles, graph, upper, u, v);
        }
    }
    triangles.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| triangle_rank(right.vertices).cmp(&triangle_rank(left.vertices)))
    });
    triangles
}

fn append_common_triangles(
    triangles: &mut Vec<FilteredTriangle>,
    graph: &Graph,
    upper: &[Vec<usize>],
    u: usize,
    v: usize,
) {
    let mut left = upper[u].partition_point(|&vertex| vertex <= v);
    let mut right = upper[v].partition_point(|&vertex| vertex <= v);
    while left < upper[u].len() && right < upper[v].len() {
        match upper[u][left].cmp(&upper[v][right]) {
            std::cmp::Ordering::Less => left += 1,
            std::cmp::Ordering::Greater => right += 1,
            std::cmp::Ordering::Equal => {
                let w = upper[u][left];
                triangles.push(FilteredTriangle {
                    vertices: [u, v, w],
                    value: graph.get(u, v).max(graph.get(u, w)).max(graph.get(v, w)),
                });
                left += 1;
                right += 1;
            }
        }
    }
}

#[derive(Clone, Default)]
struct SparseColumn(BTreeMap<usize, u64>);

impl SparseColumn {
    fn insert(&mut self, position: usize, coefficient: u64) {
        if coefficient != 0 {
            self.0.insert(position, coefficient);
        }
    }

    fn pivot(&self) -> Option<(usize, u64)> {
        self.0
            .last_key_value()
            .map(|(&position, &coefficient)| (position, coefficient))
    }

    fn add_scaled(&mut self, source: &Self, factor: u64, modulus: u64) {
        if factor == 0 {
            return;
        }
        for (&position, &coefficient) in &source.0 {
            let next =
                (self.0.get(&position).copied().unwrap_or(0) + factor * coefficient) % modulus;
            if next == 0 {
                self.0.remove(&position);
            } else {
                self.0.insert(position, next);
            }
        }
    }
}

struct CheckedReduction {
    diagram: Vec<ProofBar>,
}

fn check_reduction(
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

fn check_matrix(
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

fn check_columns(columns: &[ProofColumn], modulus: u32) -> Result<(), ProofError> {
    for (target, column) in columns.iter().enumerate() {
        check_column(target, &column.terms, modulus)?;
    }
    Ok(())
}

fn check_column(target: usize, terms: &[ProofTerm], modulus: u32) -> Result<(), ProofError> {
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

fn h0_diagram(graph: &Graph, threshold: Option<f64>) -> Result<Vec<ProofBar>, ProofError> {
    let threshold = checked_threshold(threshold)?;
    let mut edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .copied()
        .collect();
    edges.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| edge_rank([right.u, right.v]).cmp(&edge_rank([left.u, left.v])))
    });
    let mut parent: Vec<_> = (0..graph.vertex_count).collect();
    let mut diagram = Vec::new();
    for edge in edges {
        let left = find(&mut parent, edge.u);
        let right = find(&mut parent, edge.v);
        if left == right {
            continue;
        }
        parent[right] = left;
        if edge.value > 0.0 {
            diagram.push(ProofBar {
                dimension: 0,
                birth: 0.0,
                death: edge.value,
            });
        }
    }
    let roots: BTreeSet<_> = (0..graph.vertex_count)
        .map(|vertex| find(&mut parent, vertex))
        .collect();
    diagram.extend(roots.into_iter().map(|_| ProofBar {
        dimension: 0,
        birth: 0.0,
        death: f64::INFINITY,
    }));
    Ok(diagram)
}

fn find(parent: &mut [usize], mut vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[vertex] != vertex {
        let next = parent[vertex];
        parent[vertex] = root;
        vertex = next;
    }
    root
}

fn check_diagram(diagram: &[ProofBar]) -> Result<(), ProofError> {
    for bar in diagram {
        if bar.dimension > 1
            || !bar.birth.is_finite()
            || bar.birth < 0.0
            || bar.death.is_nan()
            || bar.death < 0.0
            || bar.death <= bar.birth
        {
            return Err(ProofError::new("diagram contains an invalid bar"));
        }
    }
    let mut canonical = diagram.to_vec();
    canonicalize_diagram(&mut canonical);
    if !diagrams_equal(&canonical, diagram) {
        return Err(ProofError::new("diagram bars are not canonical"));
    }
    Ok(())
}

fn canonicalize_diagram(diagram: &mut [ProofBar]) {
    diagram.sort_by(|left, right| {
        left.dimension
            .cmp(&right.dimension)
            .then(left.birth.total_cmp(&right.birth))
            .then(left.death.total_cmp(&right.death))
    });
}

fn diagrams_equal(left: &[ProofBar], right: &[ProofBar]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.dimension == right.dimension
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

fn checked_threshold(threshold: Option<f64>) -> Result<f64, ProofError> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(ProofError::new(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * v.saturating_sub(1) as u128 / 2 + u as u128
}

fn triangle_rank([u, v, w]: [usize; 3]) -> u128 {
    let choose2 = v as u128 * v.saturating_sub(1) as u128 / 2;
    let choose3 = w as u128 * w.saturating_sub(1) as u128 * w.saturating_sub(2) as u128 / 6;
    u as u128 + choose2 + choose3
}

fn is_prime(value: u64) -> bool {
    if value < 2 {
        return false;
    }
    if value % 2 == 0 {
        return value == 2;
    }
    let mut divisor = 3u64;
    while divisor <= value / divisor {
        if value % divisor == 0 {
            return false;
        }
        divisor += 2;
    }
    true
}

fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> Result<usize, ProofError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProofError::new(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(ProofError::new(format!(
            "{next} {label} exceed the limit {limit}"
        )));
    }
    Ok(next)
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

fn put_usize(output: &mut Vec<u8>, value: usize) -> Result<(), ProofError> {
    let value = u64::try_from(value)
        .map_err(|_| ProofError::new("integer does not fit the wire format"))?;
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

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProofError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(ProofError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProofError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> Result<u32, ProofError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ProofError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?).map_err(|_| ProofError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, limit: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProofError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_f64(&mut self) -> Result<Option<f64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            tag => Err(ProofError::new(format!("unknown optional-float tag {tag}"))),
        }
    }

    fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_proof() -> ProofBundle {
        let edges = vec![
            ProofEdge {
                u: 0,
                v: 1,
                value: 1.0,
            },
            ProofEdge {
                u: 0,
                v: 2,
                value: 2.0,
            },
            ProofEdge {
                u: 0,
                v: 3,
                value: 1.0,
            },
            ProofEdge {
                u: 1,
                v: 2,
                value: 1.0,
            },
            ProofEdge {
                u: 1,
                v: 3,
                value: 2.0,
            },
            ProofEdge {
                u: 2,
                v: 3,
                value: 1.0,
            },
        ];
        let graph = Graph::new(4, &edges).unwrap();
        let complex = FilteredComplex::build(&graph, None).unwrap();
        let edge_columns = reference_reduce(&complex.edge_boundaries(2), 2);
        let triangle_columns = reference_reduce(&complex.triangle_boundaries(2), 2);
        let node = AtomProof::new(
            vec![0, 1, 2, 3],
            edges.iter().map(|edge| [edge.u, edge.v]).collect(),
            edge_columns,
            triangle_columns,
        )
        .unwrap();
        let checked = check_reduction(
            &graph,
            None,
            2,
            node.edge_columns(),
            node.triangle_columns(),
        )
        .unwrap();
        let digest = *node.digest();
        let snapshot = SnapshotProof::new(4, edges, vec![digest], checked.diagram).unwrap();
        ProofBundle::new(2, None, vec![node], vec![snapshot]).unwrap()
    }

    fn reference_reduce(boundaries: &[SparseColumn], modulus: u32) -> Vec<ProofColumn> {
        let modulus = modulus as u64;
        let mut reduced: Vec<SparseColumn> = Vec::new();
        let mut basis: Vec<SparseColumn> = Vec::new();
        let mut owners: BTreeMap<usize, usize> = BTreeMap::new();
        for (target, boundary) in boundaries.iter().enumerate() {
            let mut column = boundary.clone();
            let mut transform = SparseColumn::default();
            transform.insert(target, 1);
            while let Some((pivot, coefficient)) = column.pivot() {
                let Some(&owner) = owners.get(&pivot) else {
                    break;
                };
                let owner_coefficient = reduced[owner].pivot().unwrap().1;
                let factor = (modulus
                    - coefficient * inverse_mod(owner_coefficient, modulus) % modulus)
                    % modulus;
                column.add_scaled(&reduced[owner], factor, modulus);
                transform.add_scaled(&basis[owner], factor, modulus);
            }
            if let Some((pivot, _)) = column.pivot() {
                owners.insert(pivot, target);
            }
            reduced.push(column);
            basis.push(transform);
        }
        basis
            .into_iter()
            .map(|column| ProofColumn {
                terms: column
                    .0
                    .into_iter()
                    .map(|(index, coefficient)| ProofTerm {
                        index,
                        coefficient: coefficient as u32,
                    })
                    .collect(),
            })
            .collect()
    }

    fn inverse_mod(value: u64, modulus: u64) -> u64 {
        let mut result = 1u64;
        let mut base = value;
        let mut exponent = modulus - 2;
        while exponent > 0 {
            if exponent & 1 == 1 {
                result = result * base % modulus;
            }
            base = base * base % modulus;
            exponent >>= 1;
        }
        result
    }

    #[test]
    fn proof_round_trips_and_checks() {
        let mut proof = square_proof();
        proof.snapshots.push(proof.snapshots[0].clone());
        let bytes = proof.encode().unwrap();
        let decoded = ProofBundle::decode(&bytes, ProofLimits::default()).unwrap();
        assert_eq!(decoded.encode().unwrap(), bytes);
        let verified = decoded.verify().unwrap();
        assert_eq!(verified.snapshots, 2);
        assert_eq!(verified.unique_nodes, 1);
        assert_eq!(verified.reused_references, 1);
        assert_eq!(verified.cached_references, 1);
        assert_eq!(
            verified.edge_columns_checked,
            proof.nodes[0].edge_columns.len()
        );
        assert_eq!(
            verified.triangle_columns_checked,
            proof.nodes[0].triangle_columns.len()
        );
    }

    #[test]
    fn mutation_and_arbitrary_bytes_are_rejected_without_panics() {
        let proof = square_proof();
        let bytes = proof.encode().unwrap();
        for position in [0, 8, bytes.len() / 2, bytes.len() - 1] {
            let mut changed = bytes.clone();
            changed[position] ^= 0x80;
            let result = std::panic::catch_unwind(|| {
                ProofBundle::decode(&changed, ProofLimits::default())
                    .and_then(|proof| proof.verify())
            });
            assert!(result.is_ok());
            assert!(result.unwrap().is_err());
        }
        for length in 0..bytes.len().min(256) {
            let result = std::panic::catch_unwind(|| {
                ProofBundle::decode(&bytes[..length], ProofLimits::default())
            });
            assert!(result.is_ok());
            assert!(result.unwrap().is_err());
        }
    }
}
