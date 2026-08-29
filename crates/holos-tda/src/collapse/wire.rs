//! Canonical artifacts for edge collapse.
//!
//! An artifact contains the reduced graph and its collapse certificate. It
//! binds both the thresholded input graph and the reduced graph with SHA-256.
//! Decoding checks the envelope, resource limits, graph structure, and both
//! bindings. [`crate::collapse::verify`] then checks every removal.

use std::fmt;

use sha2::{Digest, Sha256};

use super::{
    CollapseCertificate, CollapseCompleteness, CollapseObjective, CollapsedRips, RemovalStep,
    SchedulePosition,
};
use crate::SparseDistanceMatrix;

const MAGIC: &[u8; 8] = b"HOLOSCOL";
const WIRE_VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

/// Failure while constructing, encoding, or decoding a collapse artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactError {
    message: String,
}

impl ArtifactError {
    fn new(message: impl Into<String>) -> Self {
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
    matrix: SparseDistanceMatrix,
    certificate: CollapseCertificate,
    input_digest: [u8; 32],
    output_digest: [u8; 32],
}

struct ArtifactMetadata {
    algorithm_version: u32,
    objective: Option<CollapseObjective>,
    completeness: CollapseCompleteness,
    requested_threshold: Option<f64>,
    terminal_level: f64,
    work_limit: Option<u64>,
    work_used: u64,
}

struct ArtifactMetadataPrefix {
    algorithm_version: u32,
    objective: Option<CollapseObjective>,
    completeness: CollapseCompleteness,
    requested_threshold: Option<f64>,
    terminal_level: f64,
}

struct ArtifactCounts {
    vertex_count: usize,
    input_edges: usize,
    output_edges: usize,
    steps: usize,
}

struct ArtifactHeader {
    metadata: ArtifactMetadata,
    counts: ArtifactCounts,
    input_digest: [u8; 32],
    output_digest: [u8; 32],
}

struct ArtifactTail {
    counts: ArtifactCounts,
    work_limit: Option<u64>,
    work_used: u64,
    input_digest: [u8; 32],
    output_digest: [u8; 32],
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
        let output = canonical_output(&result.matrix)?;
        if output.len() != result.certificate.output_edge_count() {
            return Err(ArtifactError::new(format!(
                "certificate records {} output edges, graph has {}",
                result.certificate.output_edge_count(),
                output.len()
            )));
        }
        let input = reconstruct_input(&output, &result.certificate)?;
        Ok(Self {
            matrix: result.matrix.clone(),
            certificate: result.certificate.clone(),
            input_digest: graph_digest(result.certificate.vertex_count(), &input),
            output_digest: graph_digest(result.certificate.vertex_count(), &output),
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
        let output = canonical_output(&self.matrix)?;
        validate_stored_bindings(self, &output)?;
        let mut out = Vec::new();
        encode_header(&mut out, self, output.len())?;
        encode_output_edges(&mut out, &output)?;
        encode_steps(&mut out, self.certificate.steps())?;
        Ok(out)
    }

    /// Decode and structurally validate a canonical envelope.
    pub fn decode(bytes: &[u8], limits: DecodeLimits) -> Result<Self, ArtifactError> {
        validate_artifact_size(bytes, limits)?;
        let mut reader = Reader::new(bytes);
        decode_prefix(&mut reader)?;
        let header = decode_header(&mut reader, limits)?;
        validate_minimum_records(&reader, &header.counts)?;
        let output = decode_output_edges(&mut reader, &header.counts)?;
        let steps = decode_steps(&mut reader, &header.counts, limits)?;
        finish_decode(&reader)?;
        assemble_artifact(header, output, steps)
    }
}

fn validate_stored_bindings(
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

fn encode_header(
    out: &mut Vec<u8>,
    artifact: &CollapseArtifact,
    output_count: usize,
) -> Result<(), ArtifactError> {
    encode_prefix(out);
    encode_metadata_prefix(out, &artifact.certificate);
    encode_graph_counts(out, &artifact.certificate)?;
    put_optional_u64(out, artifact.certificate.work_limit());
    put_u64(out, artifact.certificate.work_used());
    out.extend_from_slice(&artifact.input_digest);
    out.extend_from_slice(&artifact.output_digest);
    put_usize(out, artifact.certificate.steps().len(), "step count")?;
    put_usize(out, output_count, "output edge count")
}

fn encode_prefix(out: &mut Vec<u8>) {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
}

fn encode_metadata_prefix(out: &mut Vec<u8>, certificate: &CollapseCertificate) {
    put_u32(out, certificate.algorithm_version());
    out.push(objective_tag(certificate.objective()));
    out.push(completeness_tag(certificate.completeness()));
    put_optional_f64(out, certificate.requested_threshold());
    put_u64(out, certificate.terminal_level().to_bits());
}

fn objective_tag(objective: Option<CollapseObjective>) -> u8 {
    match objective {
        None => 0,
        Some(CollapseObjective::H1) => 1,
        Some(CollapseObjective::H2) => 2,
    }
}

fn completeness_tag(completeness: CollapseCompleteness) -> u8 {
    match completeness {
        CollapseCompleteness::CompleteFixedPoint => 0,
        CollapseCompleteness::BudgetLimited => 1,
    }
}

fn encode_graph_counts(
    out: &mut Vec<u8>,
    certificate: &CollapseCertificate,
) -> Result<(), ArtifactError> {
    put_usize(out, certificate.vertex_count(), "vertex count")?;
    put_usize(out, certificate.input_edge_count(), "input edge count")?;
    put_usize(out, certificate.output_edge_count(), "output edge count")
}

fn encode_output_edges(
    out: &mut Vec<u8>,
    edges: &[(usize, usize, f64)],
) -> Result<(), ArtifactError> {
    for &(u, v, value) in edges {
        put_usize(out, u, "edge endpoint")?;
        put_usize(out, v, "edge endpoint")?;
        put_u64(out, value.to_bits());
    }
    Ok(())
}

fn encode_steps(out: &mut Vec<u8>, steps: &[RemovalStep]) -> Result<(), ArtifactError> {
    for step in steps {
        encode_step(out, step)?;
    }
    Ok(())
}

fn encode_step(out: &mut Vec<u8>, step: &RemovalStep) -> Result<(), ArtifactError> {
    let (u, v) = step.edge();
    put_usize(out, u, "step endpoint")?;
    put_usize(out, v, "step endpoint")?;
    put_u64(out, step.value().to_bits());
    let (kind, number) = encode_schedule_position(step.position());
    out.push(kind);
    put_usize(out, number, "schedule position")?;
    encode_witnesses(out, step.witnesses())
}

fn encode_schedule_position(position: SchedulePosition) -> (u8, usize) {
    match position {
        SchedulePosition::Pass(number) => (1, number),
        SchedulePosition::Round(number) => (2, number),
        SchedulePosition::Sequence(number) => (3, number),
    }
}

fn encode_witnesses(out: &mut Vec<u8>, witnesses: &[(f64, usize)]) -> Result<(), ArtifactError> {
    put_usize(out, witnesses.len(), "witness count")?;
    for &(start, apex) in witnesses {
        put_u64(out, start.to_bits());
        put_usize(out, apex, "witness apex")?;
    }
    Ok(())
}

fn validate_artifact_size(bytes: &[u8], limits: DecodeLimits) -> Result<(), ArtifactError> {
    if bytes.len() > limits.max_bytes {
        Err(ArtifactError::new(format!(
            "{} bytes exceed the decoder limit {}",
            bytes.len(),
            limits.max_bytes
        )))
    } else {
        Ok(())
    }
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ArtifactError> {
    if reader.take(8)? != MAGIC {
        return Err(ArtifactError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(ArtifactError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(ArtifactError::new(format!(
            "unsupported scalar codec {codec}"
        )));
    }
    Ok(())
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: DecodeLimits,
) -> Result<ArtifactHeader, ArtifactError> {
    let prefix = decode_metadata_prefix(reader)?;
    let tail = decode_header_tail(reader, limits)?;
    Ok(ArtifactHeader {
        metadata: ArtifactMetadata {
            algorithm_version: prefix.algorithm_version,
            objective: prefix.objective,
            completeness: prefix.completeness,
            requested_threshold: prefix.requested_threshold,
            terminal_level: prefix.terminal_level,
            work_limit: tail.work_limit,
            work_used: tail.work_used,
        },
        counts: tail.counts,
        input_digest: tail.input_digest,
        output_digest: tail.output_digest,
    })
}

fn decode_metadata_prefix(
    reader: &mut Reader<'_>,
) -> Result<ArtifactMetadataPrefix, ArtifactError> {
    Ok(ArtifactMetadataPrefix {
        algorithm_version: reader.u32()?,
        objective: decode_objective(reader.u8()?)?,
        completeness: decode_completeness(reader.u8()?)?,
        requested_threshold: reader.optional_f64()?,
        terminal_level: f64::from_bits(reader.u64()?),
    })
}

fn decode_objective(tag: u8) -> Result<Option<CollapseObjective>, ArtifactError> {
    match tag {
        0 => Ok(None),
        1 => Ok(Some(CollapseObjective::H1)),
        2 => Ok(Some(CollapseObjective::H2)),
        _ => Err(ArtifactError::new(format!("unknown objective tag {tag}"))),
    }
}

fn decode_completeness(tag: u8) -> Result<CollapseCompleteness, ArtifactError> {
    match tag {
        0 => Ok(CollapseCompleteness::CompleteFixedPoint),
        1 => Ok(CollapseCompleteness::BudgetLimited),
        _ => Err(ArtifactError::new(format!(
            "unknown completeness tag {tag}"
        ))),
    }
}

fn decode_header_tail(
    reader: &mut Reader<'_>,
    limits: DecodeLimits,
) -> Result<ArtifactTail, ArtifactError> {
    let (vertex_count, input_edges, output_edges) = decode_graph_counts(reader, limits)?;
    let work_limit = reader.optional_u64()?;
    let work_used = reader.u64()?;
    let input_digest = reader.array32()?;
    let output_digest = reader.array32()?;
    let steps = reader.bounded_usize("step count", limits.max_steps)?;
    let encoded_output = reader.bounded_usize("encoded output edge count", limits.max_edges)?;
    validate_record_counts(input_edges, output_edges, encoded_output, steps)?;
    Ok(ArtifactTail {
        counts: ArtifactCounts {
            vertex_count,
            input_edges,
            output_edges,
            steps,
        },
        work_limit,
        work_used,
        input_digest,
        output_digest,
    })
}

fn decode_graph_counts(
    reader: &mut Reader<'_>,
    limits: DecodeLimits,
) -> Result<(usize, usize, usize), ArtifactError> {
    Ok((
        reader.bounded_usize("vertex count", limits.max_vertices)?,
        reader.bounded_usize("input edge count", limits.max_edges)?,
        reader.bounded_usize("output edge count", limits.max_edges)?,
    ))
}

fn validate_record_counts(
    input: usize,
    output: usize,
    encoded_output: usize,
    steps: usize,
) -> Result<(), ArtifactError> {
    if encoded_output != output {
        return Err(ArtifactError::new(format!(
            "encoded output edge count {encoded_output} differs from header {output}"
        )));
    }
    if input != output.saturating_add(steps) {
        return Err(ArtifactError::new(format!(
            "input edge count {input} differs from output {output} plus {steps} steps"
        )));
    }
    Ok(())
}

fn validate_minimum_records(
    reader: &Reader<'_>,
    counts: &ArtifactCounts,
) -> Result<(), ArtifactError> {
    let minimum = counts
        .output_edges
        .checked_mul(24)
        .and_then(|bytes| {
            counts
                .steps
                .checked_mul(41)
                .and_then(|steps| bytes.checked_add(steps))
        })
        .ok_or_else(|| ArtifactError::new("minimum record bytes overflow usize"))?;
    if minimum > reader.remaining() {
        Err(ArtifactError::new(format!(
            "record counts need at least {minimum} bytes, only {} remain",
            reader.remaining()
        )))
    } else {
        Ok(())
    }
}

fn decode_output_edges(
    reader: &mut Reader<'_>,
    counts: &ArtifactCounts,
) -> Result<Vec<(usize, usize, f64)>, ArtifactError> {
    let mut output = Vec::with_capacity(counts.output_edges);
    for _ in 0..counts.output_edges {
        output.push((
            reader.usize()?,
            reader.usize()?,
            f64::from_bits(reader.u64()?),
        ));
    }
    validate_edges(counts.vertex_count, &output, "encoded output")?;
    Ok(output)
}

fn decode_steps(
    reader: &mut Reader<'_>,
    counts: &ArtifactCounts,
    limits: DecodeLimits,
) -> Result<Vec<RemovalStep>, ArtifactError> {
    let mut steps = Vec::with_capacity(counts.steps);
    let mut witnesses = 0usize;
    for _ in 0..counts.steps {
        steps.push(decode_step(reader, &mut witnesses, limits)?);
    }
    Ok(steps)
}

fn decode_step(
    reader: &mut Reader<'_>,
    witness_total: &mut usize,
    limits: DecodeLimits,
) -> Result<RemovalStep, ArtifactError> {
    let u = reader.usize()?;
    let v = reader.usize()?;
    let value = f64::from_bits(reader.u64()?);
    let position = decode_schedule_position(reader.u8()?, reader.usize()?)?;
    let witness_count = reader.usize()?;
    record_witness_count(reader, witness_total, witness_count, limits)?;
    Ok(RemovalStep {
        u,
        v,
        value,
        position,
        witnesses: decode_witnesses(reader, witness_count)?,
    })
}

fn decode_schedule_position(tag: u8, number: usize) -> Result<SchedulePosition, ArtifactError> {
    match tag {
        1 => Ok(SchedulePosition::Pass(number)),
        2 => Ok(SchedulePosition::Round(number)),
        3 => Ok(SchedulePosition::Sequence(number)),
        _ => Err(ArtifactError::new(format!(
            "unknown schedule-position tag {tag}"
        ))),
    }
}

fn record_witness_count(
    reader: &Reader<'_>,
    total: &mut usize,
    count: usize,
    limits: DecodeLimits,
) -> Result<(), ArtifactError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| ArtifactError::new("witness count overflows usize"))?;
    if *total > limits.max_witness_segments {
        return Err(ArtifactError::new(format!(
            "{} witness segments exceed the decoder limit {}",
            *total, limits.max_witness_segments
        )));
    }
    let bytes = count
        .checked_mul(16)
        .ok_or_else(|| ArtifactError::new("witness bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ArtifactError::new(format!(
            "{count} witness segments need {bytes} bytes, only {} remain",
            reader.remaining()
        )));
    }
    Ok(())
}

fn decode_witnesses(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<(f64, usize)>, ArtifactError> {
    (0..count)
        .map(|_| Ok((f64::from_bits(reader.u64()?), reader.usize()?)))
        .collect()
}

fn finish_decode(reader: &Reader<'_>) -> Result<(), ArtifactError> {
    if reader.remaining() == 0 {
        Ok(())
    } else {
        Err(ArtifactError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )))
    }
}

fn assemble_artifact(
    header: ArtifactHeader,
    output: Vec<(usize, usize, f64)>,
    steps: Vec<RemovalStep>,
) -> Result<CollapseArtifact, ArtifactError> {
    let matrix = SparseDistanceMatrix::from_triplets(header.counts.vertex_count, &output)
        .map_err(|error| ArtifactError::new(error.to_string()))?;
    let certificate = decoded_certificate(&header, steps);
    validate_decoded_bindings(&matrix, &certificate, &header)?;
    Ok(CollapseArtifact {
        matrix,
        certificate,
        input_digest: header.input_digest,
        output_digest: header.output_digest,
    })
}

fn decoded_certificate(header: &ArtifactHeader, steps: Vec<RemovalStep>) -> CollapseCertificate {
    CollapseCertificate {
        algorithm_version: header.metadata.algorithm_version,
        objective: header.metadata.objective,
        completeness: header.metadata.completeness,
        work_limit: header.metadata.work_limit,
        work_used: header.metadata.work_used,
        vertex_count: header.counts.vertex_count,
        requested_threshold: header.metadata.requested_threshold,
        terminal_level: header.metadata.terminal_level,
        input_edge_count: header.counts.input_edges,
        output_edge_count: header.counts.output_edges,
        steps,
    }
}

fn validate_decoded_bindings(
    matrix: &SparseDistanceMatrix,
    certificate: &CollapseCertificate,
    header: &ArtifactHeader,
) -> Result<(), ArtifactError> {
    let output = canonical_output(matrix)?;
    let input = reconstruct_input(&output, certificate)?;
    if graph_digest(header.counts.vertex_count, &input) != header.input_digest {
        return Err(ArtifactError::new("input graph binding does not match"));
    }
    if graph_digest(header.counts.vertex_count, &output) != header.output_digest {
        return Err(ArtifactError::new("output graph binding does not match"));
    }
    Ok(())
}

fn canonical_output(
    matrix: &SparseDistanceMatrix,
) -> Result<Vec<(usize, usize, f64)>, ArtifactError> {
    let edges: Vec<_> = matrix.edges().collect();
    validate_edges(matrix.len(), &edges, "output")?;
    Ok(edges)
}

fn reconstruct_input(
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

fn validate_edges(
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

pub(crate) fn graph_digest(vertices: usize, edges: &[(usize, usize, f64)]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-collapse-graph-v1");
    hash.update((vertices as u64).to_be_bytes());
    hash.update((edges.len() as u64).to_be_bytes());
    for &(u, v, value) in edges {
        hash.update((u as u64).to_be_bytes());
        hash.update((v as u64).to_be_bytes());
        hash.update(value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn put_usize(out: &mut Vec<u8>, value: usize, label: &str) -> Result<(), ArtifactError> {
    let value = u64::try_from(value)
        .map_err(|_| ArtifactError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

fn put_optional_u64(out: &mut Vec<u8>, value: Option<u64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value);
        }
    }
}

fn put_optional_f64(out: &mut Vec<u8>, value: Option<f64>) {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_u64(out, value.to_bits());
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

    fn take(&mut self, count: usize) -> Result<&'a [u8], ArtifactError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ArtifactError::new("read position overflows usize"))?;
        let Some(value) = self.bytes.get(self.position..end) else {
            return Err(ArtifactError::new(format!(
                "truncated at byte {} while reading {count} bytes",
                self.position
            )));
        };
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ArtifactError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ArtifactError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> Result<u32, ArtifactError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> Result<u64, ArtifactError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> Result<usize, ArtifactError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ArtifactError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, limit: usize) -> Result<usize, ArtifactError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ArtifactError::new(format!(
                "{label} {value} exceeds the decoder limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ArtifactError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            tag => Err(ArtifactError::new(format!(
                "unknown optional-integer tag {tag}"
            ))),
        }
    }

    fn optional_f64(&mut self) -> Result<Option<f64>, ArtifactError> {
        Ok(self.optional_u64()?.map(f64::from_bits))
    }

    fn array32(&mut self) -> Result<[u8; 32], ArtifactError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DistanceMatrix;
    use crate::collapse::verify::verify_dense_artifact;
    use crate::collapse::{
        AdaptiveCollapseParams, CollapseObjective, collapse_dense, collapse_dense_adaptive,
    };
    use proptest::prelude::*;

    fn k4() -> DistanceMatrix {
        DistanceMatrix::from_condensed(vec![1.0; 6]).unwrap()
    }

    #[test]
    fn every_certificate_version_round_trips_canonically() {
        let results = [
            collapse_dense(&k4(), None).unwrap(),
            crate::collapse::collapse_dense_rounds_parallel(&k4(), None, 2).unwrap(),
            collapse_dense_adaptive(
                &k4(),
                None,
                AdaptiveCollapseParams::new(CollapseObjective::H2),
            )
            .unwrap(),
        ];
        for result in results {
            let artifact = CollapseArtifact::from_result(&result).unwrap();
            let bytes = artifact.encode().unwrap();
            let decoded = CollapseArtifact::decode(&bytes, DecodeLimits::default()).unwrap();
            assert_eq!(decoded, artifact);
            assert_eq!(decoded.encode().unwrap(), bytes);
        }
    }

    #[test]
    fn truncation_corruption_and_trailing_bytes_are_rejected() {
        let result = collapse_dense_adaptive(
            &k4(),
            None,
            AdaptiveCollapseParams::new(CollapseObjective::H1),
        )
        .unwrap();
        let bytes = CollapseArtifact::from_result(&result)
            .unwrap()
            .encode()
            .unwrap();
        for end in 0..bytes.len() {
            assert!(CollapseArtifact::decode(&bytes[..end], DecodeLimits::default()).is_err());
        }
        let mut corrupt = bytes.clone();
        // The input binding starts after the fixed fields of this envelope.
        corrupt[59] ^= 1;
        assert!(CollapseArtifact::decode(&corrupt, DecodeLimits::default()).is_err());

        let mut trailing = bytes;
        trailing.push(0);
        let error = CollapseArtifact::decode(&trailing, DecodeLimits::default()).unwrap_err();
        assert!(error.message().contains("trailing bytes"));
    }

    #[test]
    fn noncanonical_output_records_are_rejected() {
        let matrix =
            crate::SparseDistanceMatrix::from_triplets(4, &[(0, 1, 1.0), (2, 3, 2.0)]).unwrap();
        let result = crate::collapse::collapse_sparse(&matrix, None).unwrap();
        let bytes = CollapseArtifact::from_result(&result)
            .unwrap()
            .encode()
            .unwrap();

        // The two 24-byte output records start after the 139-byte header.
        let mut permuted = bytes.clone();
        let (first, second) = permuted[139..187].split_at_mut(24);
        first.swap_with_slice(second);
        assert!(
            CollapseArtifact::decode(&permuted, DecodeLimits::default())
                .unwrap_err()
                .message()
                .contains("ascending endpoint order")
        );

        let mut negative_zero = bytes;
        negative_zero[155..163].copy_from_slice(&(-0.0f64).to_bits().to_be_bytes());
        assert!(
            CollapseArtifact::decode(&negative_zero, DecodeLimits::default())
                .unwrap_err()
                .message()
                .contains("negative zero")
        );
    }

    #[test]
    fn byte_and_collection_limits_apply_before_success() {
        let result = collapse_dense(&k4(), None).unwrap();
        let bytes = CollapseArtifact::from_result(&result)
            .unwrap()
            .encode()
            .unwrap();
        let limits = DecodeLimits {
            max_bytes: bytes.len() - 1,
            ..DecodeLimits::default()
        };
        assert!(CollapseArtifact::decode(&bytes, limits).is_err());

        let limits = DecodeLimits {
            max_vertices: 3,
            ..DecodeLimits::default()
        };
        assert!(CollapseArtifact::decode(&bytes, limits).is_err());

        let limits = DecodeLimits {
            max_steps: 0,
            ..DecodeLimits::default()
        };
        assert!(CollapseArtifact::decode(&bytes, limits).is_err());
    }

    #[test]
    fn independent_verifier_checks_the_input_binding_and_certificate() {
        let result = collapse_dense_adaptive(
            &k4(),
            None,
            AdaptiveCollapseParams::new(CollapseObjective::H2),
        )
        .unwrap();
        let artifact = CollapseArtifact::decode(
            &CollapseArtifact::from_result(&result)
                .unwrap()
                .encode()
                .unwrap(),
            DecodeLimits::default(),
        )
        .unwrap();
        verify_dense_artifact(&k4(), None, &artifact).unwrap();

        let other = DistanceMatrix::from_condensed(vec![2.0; 6]).unwrap();
        let error = verify_dense_artifact(&other, None, &artifact).unwrap_err();
        assert!(
            error.message.contains("artifact binding"),
            "{}",
            error.message
        );

        let mut changed_witness = artifact.clone();
        changed_witness.certificate.steps[0].witnesses[0].1 = 0;
        let error = verify_dense_artifact(&k4(), None, &changed_witness).unwrap_err();
        assert!(error.message.contains("apex"), "{}", error.message);
    }

    proptest! {
        #[test]
        fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = CollapseArtifact::decode(&bytes, DecodeLimits::default());
        }
    }
}
