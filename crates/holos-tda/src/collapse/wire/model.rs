use std::fmt;

use super::super::{CollapseCertificate, CollapseCompleteness, CollapseObjective, CollapsedRips};
use super::primitives::Reader;
use crate::SparseDistanceMatrix;

/// Failure while constructing, encoding, or decoding a collapse artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactError {
    message: String,
}

impl ArtifactError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated envelope rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ArtifactError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "collapse artifact: {}", self.message)
    }
}

impl std::error::Error for ArtifactError {}

/// Resource limits applied before a decoder allocates artifact collections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct DecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted vertex count.
    pub max_vertices: usize,
    /// Largest accepted input or output edge count.
    pub max_edges: usize,
    /// Largest accepted removal count.
    pub max_steps: usize,
    /// Largest accepted total witness-segment count.
    pub max_witness_segments: usize,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_vertices: 10_000_000,
            max_edges: 50_000_000,
            max_steps: 50_000_000,
            max_witness_segments: 100_000_000,
        }
    }
}

/// A reduced graph, collapse certificate, and cryptographic graph bindings.
#[derive(Debug, Clone)]
pub struct CollapseArtifact {
    pub(super) matrix: SparseDistanceMatrix,
    pub(super) certificate: CollapseCertificate,
    pub(super) input_digest: [u8; 32],
    pub(super) output_digest: [u8; 32],
}

pub(super) struct ArtifactMetadata {
    pub(super) algorithm_version: u32,
    pub(super) objective: Option<CollapseObjective>,
    pub(super) completeness: CollapseCompleteness,
    pub(super) requested_threshold: Option<f64>,
    pub(super) terminal_level: f64,
    pub(super) work_limit: Option<u64>,
    pub(super) work_used: u64,
}

pub(super) struct ArtifactMetadataPrefix {
    pub(super) algorithm_version: u32,
    pub(super) objective: Option<CollapseObjective>,
    pub(super) completeness: CollapseCompleteness,
    pub(super) requested_threshold: Option<f64>,
    pub(super) terminal_level: f64,
}

pub(super) struct ArtifactCounts {
    pub(super) vertex_count: usize,
    pub(super) input_edges: usize,
    pub(super) output_edges: usize,
    pub(super) steps: usize,
}

pub(super) struct ArtifactHeader {
    pub(super) metadata: ArtifactMetadata,
    pub(super) counts: ArtifactCounts,
    pub(super) input_digest: [u8; 32],
    pub(super) output_digest: [u8; 32],
}

pub(super) struct ArtifactTail {
    pub(super) counts: ArtifactCounts,
    pub(super) work_limit: Option<u64>,
    pub(super) work_used: u64,
    pub(super) input_digest: [u8; 32],
    pub(super) output_digest: [u8; 32],
}

impl PartialEq for CollapseArtifact {
    fn eq(&self, other: &Self) -> bool {
        self.certificate == other.certificate
            && self.input_digest == other.input_digest
            && self.output_digest == other.output_digest
            && self.matrix.len() == other.matrix.len()
            && self
                .matrix
                .edges()
                .zip(other.matrix.edges())
                .all(|(a, b)| a.0 == b.0 && a.1 == b.1 && a.2.to_bits() == b.2.to_bits())
            && self.matrix.num_edges() == other.matrix.num_edges()
    }
}

impl CollapseArtifact {
    /// Build an artifact from a collapse result.
    ///
    /// The certificate steps and output graph must reconstruct the declared
    /// input edge set without duplicates.
    pub fn from_result(result: &CollapsedRips) -> Result<Self, ArtifactError> {
        let output = super::validation::canonical_output(&result.matrix)?;
        if output.len() != result.certificate.output_edge_count() {
            return Err(ArtifactError::new(format!(
                "certificate records {} output edges, graph has {}",
                result.certificate.output_edge_count(),
                output.len()
            )));
        }
        let input = super::validation::reconstruct_input(&output, &result.certificate)?;
        Ok(Self {
            matrix: result.matrix.clone(),
            certificate: result.certificate.clone(),
            input_digest: super::primitives::graph_digest(
                result.certificate.vertex_count(),
                &input,
            ),
            output_digest: super::primitives::graph_digest(
                result.certificate.vertex_count(),
                &output,
            ),
        })
    }

    /// Reduced graph carried by the artifact.
    pub fn matrix(&self) -> &SparseDistanceMatrix {
        &self.matrix
    }

    /// Collapse certificate carried by the artifact.
    pub fn certificate(&self) -> &CollapseCertificate {
        &self.certificate
    }

    /// SHA-256 binding of the thresholded input graph.
    pub fn input_digest(&self) -> [u8; 32] {
        self.input_digest
    }

    /// SHA-256 binding of the reduced graph.
    pub fn output_digest(&self) -> [u8; 32] {
        self.output_digest
    }

    /// Encode the canonical wire version 1 envelope.
    pub fn encode(&self) -> Result<Vec<u8>, ArtifactError> {
        let output = super::validation::canonical_output(&self.matrix)?;
        super::validation::validate_stored_bindings(self, &output)?;
        let mut out = Vec::new();
        super::encode::encode_header(&mut out, self, output.len())?;
        super::encode::encode_output_edges(&mut out, &output)?;
        super::encode::encode_steps(&mut out, self.certificate.steps())?;
        Ok(out)
    }

    /// Decode and structurally validate a canonical envelope.
    pub fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<Self, ArtifactError> {
        super::decode::validate_artifact_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        super::decode::decode_prefix(&mut reader)?;
        let header = super::decode::decode_header(&mut reader, limits)?;
        super::decode::validate_minimum_records(&reader, &header.counts)?;
        let output = super::decode::decode_output_edges(&mut reader, &header.counts)?;
        let steps = super::decode::decode_steps(&mut reader, &header.counts, limits)?;
        super::decode::finish_decode(&reader)?;
        super::decode::assemble_artifact(header, output, steps)
    }
}
