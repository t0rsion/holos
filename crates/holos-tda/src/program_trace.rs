//! Portable independently checked traces of persistence-program updates.
//!
//! A `HOLOSDLT` envelope is self-contained. It stores the initial graph and
//! program, every updated graph, exact work and continuation records, and a
//! new program checkpoint only when certified reuse is not possible.

use std::fmt;

use crate::classes::{BasisClassId, IntervalGroupId};
use crate::program::class_continuation;
use crate::{
    Bar, BasisTransport, CertificateLimits, ClassContinuation, ClassCorrespondence,
    ContinuationKind, CorrespondenceTerm, CorrespondenceVector, Diagram, EdgeKey, Error,
    PersistenceProgram, ProgramArtifact, ProgramArtifactError, ProgramDecodeLimits, ProgramEvent,
    ProgramEventKind, ProgramUpdateMode, ProgramWork, ReductionGuardKind, RipsParams,
    SparseDistanceMatrix,
};

const MAGIC: &[u8; 8] = b"HOLOSDLT";
const WIRE_VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;

/// Failure while producing, decoding, or checking a program trace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramTraceError {
    message: String,
}

impl ProgramTraceError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Description of the violated trace rule.
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for ProgramTraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "program trace: {}", self.message)
    }
}

impl std::error::Error for ProgramTraceError {}

/// Decoder limits applied before trace collections are allocated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProgramTraceDecodeLimits {
    /// Largest accepted envelope in bytes.
    pub max_bytes: usize,
    /// Largest accepted update count.
    pub max_steps: usize,
    /// Largest accepted vertices in one graph.
    pub max_vertices: usize,
    /// Largest accepted edges across all embedded graphs.
    pub max_total_edges: usize,
    /// Largest accepted event count across all steps.
    pub max_events: usize,
    /// Largest accepted continuation-record count across all steps.
    pub max_continuations: usize,
    /// Largest accepted basis-transport count across all steps.
    pub max_transports: usize,
    /// Largest accepted correspondence-record count across all steps.
    pub max_correspondences: usize,
    /// Largest accepted correspondence-vector count across all steps.
    pub max_correspondence_vectors: usize,
    /// Largest accepted correspondence-term count across all steps.
    pub max_correspondence_terms: usize,
    /// Largest accepted bars across all step diagrams.
    pub max_bars: usize,
    /// Largest accepted bytes across all program checkpoints.
    pub max_checkpoint_bytes: usize,
    /// Limits for each nested program artifact.
    pub program: ProgramDecodeLimits,
}

impl Default for ProgramTraceDecodeLimits {
    fn default() -> Self {
        Self {
            max_bytes: 1 << 30,
            max_steps: 10_000_000,
            max_vertices: 1_000_000,
            max_total_edges: 200_000_000,
            max_events: 100_000_000,
            max_continuations: 100_000_000,
            max_transports: 200_000_000,
            max_correspondences: 100_000_000,
            max_correspondence_vectors: 200_000_000,
            max_correspondence_terms: 400_000_000,
            max_bars: 100_000_000,
            max_checkpoint_bytes: 1 << 30,
            program: ProgramDecodeLimits::default(),
        }
    }
}

/// One graph update and its declared checked result.
#[derive(Debug, Clone)]
pub struct ProgramTraceStep {
    graph: SparseDistanceMatrix,
    mode: ProgramUpdateMode,
    work: ProgramWork,
    events: Vec<ProgramEvent>,
    continuation: Vec<ClassContinuation>,
    correspondence: Vec<ClassCorrespondence>,
    diagram: Diagram,
    checkpoint: Option<ProgramArtifact>,
}

impl ProgramTraceStep {
    /// Updated graph embedded in the trace.
    pub fn graph(&self) -> &SparseDistanceMatrix {
        &self.graph
    }

    /// Declared execution mode.
    pub fn mode(&self) -> ProgramUpdateMode {
        self.mode
    }

    /// Exact work charged by the producer.
    pub fn work(&self) -> ProgramWork {
        self.work
    }

    /// Declared topology and algebraic events.
    pub fn events(&self) -> &[ProgramEvent] {
        &self.events
    }

    /// Declared class-space continuation records.
    pub fn continuation(&self) -> &[ClassContinuation] {
        &self.continuation
    }

    /// Exact class-space relations on common filtered subcomplexes.
    pub fn correspondence(&self) -> &[ClassCorrespondence] {
        &self.correspondence
    }

    /// Exact diagram at this step.
    pub fn diagram(&self) -> &Diagram {
        &self.diagram
    }

    /// New full checkpoint, present only after repair or recompilation.
    pub fn checkpoint(&self) -> Option<&ProgramArtifact> {
        self.checkpoint.as_ref()
    }
}

/// Self-contained sequence of proof-carrying program updates.
#[derive(Debug, Clone)]
pub struct ProgramTraceArtifact {
    initial_graph: SparseDistanceMatrix,
    initial_program: ProgramArtifact,
    steps: Vec<ProgramTraceStep>,
}

impl ProgramTraceArtifact {
    /// Produce a checked trace from an initial graph and updated graphs.
    pub fn build(
        initial: &SparseDistanceMatrix,
        updates: &[SparseDistanceMatrix],
        params: &RipsParams,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramTraceError> {
        let (initial_program, mut program) =
            ProgramArtifact::compile(initial, params, certificate_limits)
                .map_err(program_artifact_error)?;
        let mut steps = Vec::with_capacity(updates.len());
        for graph in updates {
            let update = program
                .advance(graph)
                .map_err(|error| ProgramTraceError::new(error.to_string()))?;
            let checkpoint = (update.mode != ProgramUpdateMode::Reused)
                .then(|| ProgramArtifact::capture(graph, &program))
                .transpose()
                .map_err(program_artifact_error)?;
            steps.push(ProgramTraceStep {
                graph: graph.clone(),
                mode: update.mode,
                work: update.work,
                events: update.events,
                continuation: update.continuation,
                correspondence: update.correspondence,
                diagram: update.result.diagram,
                checkpoint,
            });
        }
        Ok(Self {
            initial_graph: initial.clone(),
            initial_program,
            steps,
        })
    }

    /// Initial graph embedded in the trace.
    pub fn initial_graph(&self) -> &SparseDistanceMatrix {
        &self.initial_graph
    }

    /// Initial independently checked program.
    pub fn initial_program(&self) -> &ProgramArtifact {
        &self.initial_program
    }

    /// Ordered update steps.
    pub fn steps(&self) -> &[ProgramTraceStep] {
        &self.steps
    }

    /// Encode the canonical `HOLOSDLT` version 2 envelope.
    pub fn encode(&self) -> std::result::Result<Vec<u8>, ProgramTraceError> {
        let initial_program = encode_program_artifact(&self.initial_program)?;
        let checkpoints = encode_checkpoints(&self.steps)?;
        let mut out = Vec::new();
        encode_trace_header(&mut out, self.steps.len(), initial_program.len())?;
        encode_graph(&mut out, &self.initial_graph)?;
        out.extend_from_slice(&initial_program);
        for (step, checkpoint) in self.steps.iter().zip(checkpoints) {
            encode_trace_step(&mut out, step, checkpoint.as_deref())?;
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded program trace.
    pub fn decode(
        bytes: &[u8],
        limits: ProgramTraceDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramTraceError> {
        check_envelope_size(bytes, limits.max_bytes)?;
        let mut reader = Reader::new(bytes);
        let header = decode_trace_header(&mut reader, limits)?;
        let mut total_edges = 0usize;
        let initial_graph = decode_graph(&mut reader, limits, &mut total_edges)?;
        let initial_program = decode_program_artifact(
            &mut reader,
            header.initial_program_bytes,
            limits,
            certificate_limits,
        )?;
        let mut totals = TraceTotals {
            checkpoint_bytes: header.initial_program_bytes,
            ..TraceTotals::default()
        };
        let steps = decode_trace_steps(
            &mut reader,
            header.step_count,
            limits,
            certificate_limits,
            &mut totals,
            &mut total_edges,
        )?;
        check_no_trailing_bytes(&reader)?;
        Ok(Self {
            initial_graph,
            initial_program,
            steps,
        })
    }

    /// Verify every reused step and checkpoint without the persistence solver.
    pub fn verify(
        &self,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<VerifiedProgramTrace, ProgramTraceError> {
        let mut program = verify_program_artifact(
            &self.initial_program,
            &self.initial_graph,
            certificate_limits,
        )?;
        let initial_result = program.result().clone();
        let mut verified_steps = Vec::with_capacity(self.steps.len());
        for (index, step) in self.steps.iter().enumerate() {
            check_step_shape(step)?;
            let checked = replay_step(&mut program, step, index, certificate_limits)?;
            check_replayed_step(&checked, step, index)?;
            verified_steps.push(checked.into());
        }
        Ok(VerifiedProgramTrace {
            initial_result,
            steps: verified_steps,
            final_program: program,
        })
    }
}

/// One independently replayed program step.
#[derive(Debug, Clone)]
pub struct VerifiedProgramTraceStep {
    /// Checked execution mode.
    pub mode: ProgramUpdateMode,
    /// Checked work counts.
    pub work: ProgramWork,
    /// Checked events.
    pub events: Vec<ProgramEvent>,
    /// Checked class-space continuation.
    pub continuation: Vec<ClassContinuation>,
    /// Checked exact class-space correspondence.
    pub correspondence: Vec<ClassCorrespondence>,
    /// Checked exact diagram.
    pub diagram: Diagram,
}

/// Result of independently checking a complete program trace.
#[derive(Debug, Clone)]
pub struct VerifiedProgramTrace {
    /// Checked initial diagram and H1 class spaces.
    pub initial_result: crate::ExplainedDiagram,
    /// Checked update steps.
    pub steps: Vec<VerifiedProgramTraceStep>,
    /// Ready program at the final graph.
    pub final_program: PersistenceProgram,
}

#[derive(Default)]
struct TraceTotals {
    events: usize,
    continuations: usize,
    transports: usize,
    correspondences: usize,
    correspondence_vectors: usize,
    correspondence_terms: usize,
    bars: usize,
    checkpoint_bytes: usize,
}

struct TraceHeader {
    step_count: usize,
    initial_program_bytes: usize,
}

struct StepCounts {
    events: usize,
    continuations: usize,
    correspondences: usize,
    bars: usize,
    checkpoint_bytes: usize,
}

struct DecodedStepBody {
    events: Vec<ProgramEvent>,
    continuation: Vec<ClassContinuation>,
    correspondence: Vec<ClassCorrespondence>,
    diagram: Diagram,
}

struct CorrespondenceHeader {
    old_space: IntervalGroupId,
    new_space: IntervalGroupId,
    scale: f64,
    old_rank: usize,
    new_rank: usize,
    old_image_rank: usize,
    new_image_rank: usize,
    relation_rank: usize,
    basis_count: usize,
}

impl TraceTotals {
    fn add_step(
        &mut self,
        counts: &StepCounts,
        limits: ProgramTraceDecodeLimits,
    ) -> std::result::Result<(), ProgramTraceError> {
        self.events = bounded_sum(self.events, counts.events, limits.max_events, "events")?;
        self.continuations = bounded_sum(
            self.continuations,
            counts.continuations,
            limits.max_continuations,
            "continuations",
        )?;
        self.correspondences = bounded_sum(
            self.correspondences,
            counts.correspondences,
            limits.max_correspondences,
            "correspondences",
        )?;
        self.bars = bounded_sum(self.bars, counts.bars, limits.max_bars, "bars")?;
        self.checkpoint_bytes = bounded_sum(
            self.checkpoint_bytes,
            counts.checkpoint_bytes,
            limits.max_checkpoint_bytes,
            "checkpoint bytes",
        )?;
        Ok(())
    }
}

impl From<crate::ProgramUpdate> for VerifiedProgramTraceStep {
    fn from(update: crate::ProgramUpdate) -> Self {
        Self {
            mode: update.mode,
            work: update.work,
            events: update.events,
            continuation: update.continuation,
            correspondence: update.correspondence,
            diagram: update.result.diagram,
        }
    }
}

fn encode_program_artifact(
    artifact: &ProgramArtifact,
) -> std::result::Result<Vec<u8>, ProgramTraceError> {
    artifact.encode().map_err(program_artifact_error)
}

fn encode_checkpoints(
    steps: &[ProgramTraceStep],
) -> std::result::Result<Vec<Option<Vec<u8>>>, ProgramTraceError> {
    let mut encoded = Vec::with_capacity(steps.len());
    for step in steps {
        encoded.push(
            step.checkpoint
                .as_ref()
                .map(encode_program_artifact)
                .transpose()?,
        );
    }
    Ok(encoded)
}

fn encode_trace_header(
    out: &mut Vec<u8>,
    step_count: usize,
    initial_program_bytes: usize,
) -> std::result::Result<(), ProgramTraceError> {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
    put_usize(out, step_count, "step count")?;
    put_usize(out, initial_program_bytes, "initial program byte count")?;
    Ok(())
}

fn encode_trace_step(
    out: &mut Vec<u8>,
    step: &ProgramTraceStep,
    checkpoint: Option<&[u8]>,
) -> std::result::Result<(), ProgramTraceError> {
    encode_step_header(out, step, checkpoint.map_or(0, <[u8]>::len))?;
    encode_graph(out, &step.graph)?;
    encode_step_body(out, step)?;
    if let Some(checkpoint) = checkpoint {
        out.extend_from_slice(checkpoint);
    }
    Ok(())
}

fn encode_step_header(
    out: &mut Vec<u8>,
    step: &ProgramTraceStep,
    checkpoint_bytes: usize,
) -> std::result::Result<(), ProgramTraceError> {
    out.push(mode_tag(step.mode));
    encode_work(out, step.work)?;
    put_usize(out, step.events.len(), "event count")?;
    put_usize(out, step.continuation.len(), "continuation count")?;
    put_usize(out, step.correspondence.len(), "correspondence count")?;
    put_usize(out, step.diagram.bars.len(), "bar count")?;
    put_usize(out, checkpoint_bytes, "checkpoint byte count")?;
    Ok(())
}

fn encode_step_body(
    out: &mut Vec<u8>,
    step: &ProgramTraceStep,
) -> std::result::Result<(), ProgramTraceError> {
    encode_events(out, &step.events)?;
    encode_continuations(out, &step.continuation)?;
    encode_correspondences(out, &step.correspondence)?;
    encode_diagram(out, &step.diagram)
}

fn encode_events(
    out: &mut Vec<u8>,
    events: &[ProgramEvent],
) -> std::result::Result<(), ProgramTraceError> {
    for event in events {
        encode_event(out, event)?;
    }
    Ok(())
}

fn encode_continuations(
    out: &mut Vec<u8>,
    continuations: &[ClassContinuation],
) -> std::result::Result<(), ProgramTraceError> {
    for continuation in continuations {
        encode_continuation(out, continuation)?;
    }
    Ok(())
}

fn encode_correspondences(
    out: &mut Vec<u8>,
    correspondences: &[ClassCorrespondence],
) -> std::result::Result<(), ProgramTraceError> {
    for correspondence in correspondences {
        encode_correspondence(out, correspondence)?;
    }
    Ok(())
}

fn check_envelope_size(bytes: &[u8], max_bytes: usize) -> Result<(), ProgramTraceError> {
    if bytes.len() > max_bytes {
        return Err(ProgramTraceError::new(format!(
            "{} bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(())
}

fn decode_trace_header(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
) -> std::result::Result<TraceHeader, ProgramTraceError> {
    check_trace_identity(reader)?;
    Ok(TraceHeader {
        step_count: reader.bounded_usize("step count", limits.max_steps)?,
        initial_program_bytes: reader
            .bounded_usize("initial program byte count", limits.max_checkpoint_bytes)?,
    })
}

fn check_trace_identity(reader: &mut Reader<'_>) -> Result<(), ProgramTraceError> {
    if reader.take(8)? != MAGIC {
        return Err(ProgramTraceError::new("wrong magic bytes"));
    }
    if reader.u16()? != WIRE_VERSION {
        return Err(ProgramTraceError::new("unsupported wire version"));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(ProgramTraceError::new("unsupported scalar codec"));
    }
    Ok(())
}

fn decode_program_artifact(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: ProgramTraceDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<ProgramArtifact, ProgramTraceError> {
    ProgramArtifact::decode(reader.take(byte_count)?, limits.program, certificate_limits)
        .map_err(program_artifact_error)
}

fn decode_trace_steps(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    certificate_limits: CertificateLimits,
    totals: &mut TraceTotals,
    total_edges: &mut usize,
) -> Result<Vec<ProgramTraceStep>, ProgramTraceError> {
    let mut steps = Vec::with_capacity(count);
    for _ in 0..count {
        steps.push(decode_trace_step(
            reader,
            limits,
            certificate_limits,
            totals,
            total_edges,
        )?);
    }
    Ok(steps)
}

fn decode_trace_step(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    certificate_limits: CertificateLimits,
    totals: &mut TraceTotals,
    total_edges: &mut usize,
) -> Result<ProgramTraceStep, ProgramTraceError> {
    let mode = decode_mode(reader.u8()?)?;
    let work = decode_work(reader)?;
    let counts = decode_step_counts(reader)?;
    totals.add_step(&counts, limits)?;
    let graph = decode_graph(reader, limits, total_edges)?;
    let body = decode_step_body(reader, &counts, limits, totals)?;
    let checkpoint =
        decode_checkpoint(reader, counts.checkpoint_bytes, limits, certificate_limits)?;
    let step = ProgramTraceStep {
        graph,
        mode,
        work,
        events: body.events,
        continuation: body.continuation,
        correspondence: body.correspondence,
        diagram: body.diagram,
        checkpoint,
    };
    check_step_shape(&step)?;
    Ok(step)
}

fn decode_step_counts(reader: &mut Reader<'_>) -> Result<StepCounts, ProgramTraceError> {
    Ok(StepCounts {
        events: reader.usize()?,
        continuations: reader.usize()?,
        correspondences: reader.usize()?,
        bars: reader.usize()?,
        checkpoint_bytes: reader.usize()?,
    })
}

fn decode_step_body(
    reader: &mut Reader<'_>,
    counts: &StepCounts,
    limits: ProgramTraceDecodeLimits,
    totals: &mut TraceTotals,
) -> Result<DecodedStepBody, ProgramTraceError> {
    Ok(DecodedStepBody {
        events: decode_events(reader, counts.events)?,
        continuation: decode_continuations(reader, counts.continuations, limits, totals)?,
        correspondence: decode_correspondences(reader, counts.correspondences, limits, totals)?,
        diagram: decode_diagram(reader, counts.bars)?,
    })
}

fn decode_events(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<ProgramEvent>, ProgramTraceError> {
    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        events.push(decode_event(reader)?);
    }
    Ok(events)
}

fn decode_continuations(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    totals: &mut TraceTotals,
) -> Result<Vec<ClassContinuation>, ProgramTraceError> {
    let mut continuations = Vec::with_capacity(count);
    for _ in 0..count {
        continuations.push(decode_continuation(reader, limits, &mut totals.transports)?);
    }
    Ok(continuations)
}

fn decode_correspondences(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    totals: &mut TraceTotals,
) -> Result<Vec<ClassCorrespondence>, ProgramTraceError> {
    let mut correspondences = Vec::with_capacity(count);
    for _ in 0..count {
        correspondences.push(decode_correspondence(reader, limits, totals)?);
    }
    Ok(correspondences)
}

fn decode_checkpoint(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: ProgramTraceDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<Option<ProgramArtifact>, ProgramTraceError> {
    if byte_count == 0 {
        return Ok(None);
    }
    decode_program_artifact(reader, byte_count, limits, certificate_limits).map(Some)
}

fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), ProgramTraceError> {
    if reader.remaining() != 0 {
        return Err(ProgramTraceError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}

fn verify_program_artifact(
    artifact: &ProgramArtifact,
    graph: &SparseDistanceMatrix,
    certificate_limits: CertificateLimits,
) -> Result<PersistenceProgram, ProgramTraceError> {
    artifact
        .verify(graph, certificate_limits)
        .map_err(program_artifact_error)
}

fn replay_step(
    program: &mut PersistenceProgram,
    step: &ProgramTraceStep,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<crate::ProgramUpdate, ProgramTraceError> {
    if step.mode == ProgramUpdateMode::Reused {
        return program
            .advance_reused(&step.graph)
            .map_err(|error| ProgramTraceError::new(error.to_string()));
    }
    replay_checkpoint_step(program, step, index, certificate_limits)
}

fn replay_checkpoint_step(
    program: &mut PersistenceProgram,
    step: &ProgramTraceStep,
    index: usize,
    certificate_limits: CertificateLimits,
) -> Result<crate::ProgramUpdate, ProgramTraceError> {
    let checkpoint = step.checkpoint.as_ref().ok_or_else(|| {
        ProgramTraceError::new(format!("step {index} has no required checkpoint"))
    })?;
    let replacement = verify_program_artifact(checkpoint, &step.graph, certificate_limits)?;
    let (mode, events, work) = program
        .preview_update(&step.graph, replacement.states().len())
        .map_err(|error| ProgramTraceError::new(error.to_string()))?;
    let continuation = class_continuation(&program.result().spaces, &replacement.result().spaces);
    let correspondence = replay_correspondence(program, &step.graph, &replacement)?;
    let result = replacement.result().clone();
    *program = replacement;
    Ok(crate::ProgramUpdate {
        result,
        mode,
        events,
        continuation,
        correspondence,
        work,
    })
}

fn replay_correspondence(
    program: &PersistenceProgram,
    graph: &SparseDistanceMatrix,
    replacement: &PersistenceProgram,
) -> Result<Vec<ClassCorrespondence>, ProgramTraceError> {
    crate::class_correspondences(
        program.current_graph(),
        &program.result().spaces,
        graph,
        &replacement.result().spaces,
        replacement.params().modulus,
    )
    .map_err(|error| ProgramTraceError::new(error.to_string()))
}

fn check_replayed_step(
    checked: &crate::ProgramUpdate,
    declared: &ProgramTraceStep,
    index: usize,
) -> Result<(), ProgramTraceError> {
    let differs = checked.mode != declared.mode
        || checked.work != declared.work
        || checked.events != declared.events
        || checked.continuation != declared.continuation
        || checked.correspondence != declared.correspondence
        || !diagram_bits_equal(&checked.result.diagram, &declared.diagram);
    if differs {
        return Err(ProgramTraceError::new(format!(
            "step {index} differs from independent replay"
        )));
    }
    Ok(())
}

fn check_step_shape(step: &ProgramTraceStep) -> std::result::Result<(), ProgramTraceError> {
    match (step.mode, step.checkpoint.is_some()) {
        (ProgramUpdateMode::Reused, false)
        | (ProgramUpdateMode::Repaired, true)
        | (ProgramUpdateMode::Recompiled, true) => Ok(()),
        (ProgramUpdateMode::Reused, true) => Err(ProgramTraceError::new(
            "a reused step contains a redundant checkpoint",
        )),
        (_, false) => Err(ProgramTraceError::new(
            "a repaired or recompiled step has no checkpoint",
        )),
    }
}

fn encode_graph(
    out: &mut Vec<u8>,
    graph: &SparseDistanceMatrix,
) -> std::result::Result<(), ProgramTraceError> {
    put_usize(out, graph.len(), "graph vertex count")?;
    put_usize(out, graph.num_edges(), "graph edge count")?;
    for (u, v, value) in graph.edges() {
        put_usize(out, u, "graph edge endpoint")?;
        put_usize(out, v, "graph edge endpoint")?;
        put_u64(out, value.to_bits());
    }
    Ok(())
}

fn decode_graph(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    total_edges: &mut usize,
) -> std::result::Result<SparseDistanceMatrix, ProgramTraceError> {
    let vertices = reader.bounded_usize("graph vertex count", limits.max_vertices)?;
    let edges = reader.usize()?;
    *total_edges = bounded_sum(
        *total_edges,
        edges,
        limits.max_total_edges,
        "embedded graph edges",
    )?;
    let bytes = edges
        .checked_mul(24)
        .ok_or_else(|| ProgramTraceError::new("graph record bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(
            "graph record exceeds the remaining bytes",
        ));
    }
    let mut triplets = Vec::with_capacity(edges);
    for _ in 0..edges {
        triplets.push((
            reader.usize()?,
            reader.usize()?,
            f64::from_bits(reader.u64()?),
        ));
    }
    SparseDistanceMatrix::from_triplets(vertices, &triplets)
        .map_err(|error| ProgramTraceError::new(error.to_string()))
}

fn encode_work(out: &mut Vec<u8>, work: ProgramWork) -> std::result::Result<(), ProgramTraceError> {
    for (value, label) in [
        (work.edges_checked, "edges checked"),
        (work.h0_edges_scanned, "H0 edges scanned"),
        (work.guards_checked, "guards checked"),
        (work.atoms_touched, "atoms touched"),
        (work.atoms_reused, "atoms reused"),
        (work.atoms_repaired, "atoms repaired"),
        (work.atoms_rebuilt, "atoms rebuilt"),
        (work.reduction_columns_reused, "reduction columns reused"),
        (work.reduction_columns_reduced, "reduction columns reduced"),
        (
            work.reduction_column_additions,
            "reduction column additions",
        ),
    ] {
        put_usize(out, value, label)?;
    }
    Ok(())
}

fn decode_work(reader: &mut Reader<'_>) -> std::result::Result<ProgramWork, ProgramTraceError> {
    let leading = decode_work_leading(reader)?;
    let reduction = decode_work_reduction(reader)?;
    Ok(ProgramWork {
        edges_checked: leading[0],
        h0_edges_scanned: leading[1],
        guards_checked: leading[2],
        atoms_touched: leading[3],
        atoms_reused: leading[4],
        atoms_repaired: reduction[0],
        atoms_rebuilt: reduction[1],
        reduction_columns_reused: reduction[2],
        reduction_columns_reduced: reduction[3],
        reduction_column_additions: reduction[4],
    })
}

fn decode_work_leading(reader: &mut Reader<'_>) -> Result<[usize; 5], ProgramTraceError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

fn decode_work_reduction(reader: &mut Reader<'_>) -> Result<[usize; 5], ProgramTraceError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

fn encode_event(
    out: &mut Vec<u8>,
    event: &ProgramEvent,
) -> std::result::Result<(), ProgramTraceError> {
    out.push(event_kind_tag(event.kind));
    put_optional_usize(out, event.atom, "event atom")?;
    put_optional_edge(out, event.edge)?;
    out.push(match event.guard {
        None => 0,
        Some(ReductionGuardKind::ChangeOfBasis) => 1,
        Some(ReductionGuardKind::Pivot) => 2,
    });
    Ok(())
}

fn decode_event(reader: &mut Reader<'_>) -> std::result::Result<ProgramEvent, ProgramTraceError> {
    let kind = decode_event_kind(reader.u8()?)?;
    let atom = reader.optional_usize()?;
    let edge = reader.optional_edge()?;
    let guard = match reader.u8()? {
        0 => None,
        1 => Some(ReductionGuardKind::ChangeOfBasis),
        2 => Some(ReductionGuardKind::Pivot),
        tag => {
            return Err(ProgramTraceError::new(format!(
                "unknown guard-kind tag {tag}"
            )));
        }
    };
    Ok(ProgramEvent {
        kind,
        atom,
        edge,
        guard,
    })
}

fn encode_continuation(
    out: &mut Vec<u8>,
    continuation: &ClassContinuation,
) -> std::result::Result<(), ProgramTraceError> {
    out.push(continuation_kind_tag(continuation.kind));
    put_usize(out, continuation.old_spaces.len(), "old-space count")?;
    put_usize(out, continuation.new_spaces.len(), "new-space count")?;
    put_usize(out, continuation.transport.len(), "transport count")?;
    for id in &continuation.old_spaces {
        out.extend_from_slice(id.as_bytes());
    }
    for id in &continuation.new_spaces {
        out.extend_from_slice(id.as_bytes());
    }
    for transport in &continuation.transport {
        out.extend_from_slice(transport.old.as_bytes());
        out.extend_from_slice(transport.new.as_bytes());
        put_u32(out, transport.coefficient);
    }
    Ok(())
}

fn decode_continuation(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    total_transports: &mut usize,
) -> std::result::Result<ClassContinuation, ProgramTraceError> {
    let kind = decode_continuation_kind(reader.u8()?)?;
    let [old_count, new_count, transport_count] = decode_continuation_counts(reader)?;
    *total_transports = bounded_sum(
        *total_transports,
        transport_count,
        limits.max_transports,
        "basis transports",
    )?;
    check_continuation_bytes(reader, old_count, new_count, transport_count)?;
    let old_spaces = decode_interval_group_ids(reader, old_count)?;
    let new_spaces = decode_interval_group_ids(reader, new_count)?;
    let transport = decode_basis_transports(reader, transport_count)?;
    Ok(ClassContinuation {
        kind,
        old_spaces,
        new_spaces,
        transport,
    })
}

fn decode_continuation_counts(reader: &mut Reader<'_>) -> Result<[usize; 3], ProgramTraceError> {
    Ok([reader.usize()?, reader.usize()?, reader.usize()?])
}

fn check_continuation_bytes(
    reader: &Reader<'_>,
    old_count: usize,
    new_count: usize,
    transport_count: usize,
) -> Result<(), ProgramTraceError> {
    let id_bytes = old_count
        .checked_add(new_count)
        .and_then(|count| count.checked_mul(32))
        .ok_or_else(|| ProgramTraceError::new("continuation bytes overflow usize"))?;
    let transport_bytes = transport_count
        .checked_mul(68)
        .ok_or_else(|| ProgramTraceError::new("continuation bytes overflow usize"))?;
    let bytes = id_bytes
        .checked_add(transport_bytes)
        .ok_or_else(|| ProgramTraceError::new("continuation bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(
            "continuation exceeds the remaining bytes",
        ));
    }
    Ok(())
}

fn decode_interval_group_ids(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<IntervalGroupId>, ProgramTraceError> {
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        ids.push(IntervalGroupId::from_bytes(reader.array32()?));
    }
    Ok(ids)
}

fn decode_basis_transports(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<BasisTransport>, ProgramTraceError> {
    let mut transports = Vec::with_capacity(count);
    for _ in 0..count {
        transports.push(BasisTransport {
            old: BasisClassId::from_bytes(reader.array32()?),
            new: BasisClassId::from_bytes(reader.array32()?),
            coefficient: reader.u32()?,
        });
    }
    Ok(transports)
}

fn encode_correspondence(
    out: &mut Vec<u8>,
    correspondence: &ClassCorrespondence,
) -> std::result::Result<(), ProgramTraceError> {
    out.extend_from_slice(correspondence.old_space.as_bytes());
    out.extend_from_slice(correspondence.new_space.as_bytes());
    put_u64(out, correspondence.scale.to_bits());
    for (value, label) in [
        (correspondence.old_rank, "old rank"),
        (correspondence.new_rank, "new rank"),
        (correspondence.old_image_rank, "old image rank"),
        (correspondence.new_image_rank, "new image rank"),
        (correspondence.relation_rank, "relation rank"),
        (correspondence.basis.len(), "correspondence basis count"),
    ] {
        put_usize(out, value, label)?;
    }
    for vector in &correspondence.basis {
        put_usize(out, vector.old.len(), "old correspondence term count")?;
        put_usize(out, vector.new.len(), "new correspondence term count")?;
        encode_correspondence_terms(out, &vector.old);
        encode_correspondence_terms(out, &vector.new);
    }
    Ok(())
}

fn encode_correspondence_terms(out: &mut Vec<u8>, terms: &[CorrespondenceTerm]) {
    for term in terms {
        out.extend_from_slice(term.basis.as_bytes());
        put_u32(out, term.coefficient);
    }
}

fn decode_correspondence(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    totals: &mut TraceTotals,
) -> std::result::Result<ClassCorrespondence, ProgramTraceError> {
    let header = decode_correspondence_header(reader)?;
    totals.correspondence_vectors = bounded_sum(
        totals.correspondence_vectors,
        header.basis_count,
        limits.max_correspondence_vectors,
        "correspondence vectors",
    )?;
    let basis = decode_correspondence_basis(reader, header.basis_count, limits, totals)?;
    Ok(ClassCorrespondence {
        old_space: header.old_space,
        new_space: header.new_space,
        scale: header.scale,
        old_rank: header.old_rank,
        new_rank: header.new_rank,
        old_image_rank: header.old_image_rank,
        new_image_rank: header.new_image_rank,
        relation_rank: header.relation_rank,
        basis,
    })
}

fn decode_correspondence_header(
    reader: &mut Reader<'_>,
) -> Result<CorrespondenceHeader, ProgramTraceError> {
    let old_space = IntervalGroupId::from_bytes(reader.array32()?);
    let new_space = IntervalGroupId::from_bytes(reader.array32()?);
    let scale = f64::from_bits(reader.u64()?);
    check_correspondence_scale(scale)?;
    let ranks = decode_correspondence_ranks(reader)?;
    let header = CorrespondenceHeader {
        old_space,
        new_space,
        scale,
        old_rank: ranks[0],
        new_rank: ranks[1],
        old_image_rank: ranks[2],
        new_image_rank: ranks[3],
        relation_rank: ranks[4],
        basis_count: ranks[5],
    };
    check_correspondence_ranks(&header)?;
    Ok(header)
}

fn check_correspondence_scale(scale: f64) -> Result<(), ProgramTraceError> {
    if !scale.is_finite() || scale < 0.0 {
        return Err(ProgramTraceError::new(
            "correspondence scale must be finite and non-negative",
        ));
    }
    Ok(())
}

fn decode_correspondence_ranks(reader: &mut Reader<'_>) -> Result<[usize; 6], ProgramTraceError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

fn check_correspondence_ranks(header: &CorrespondenceHeader) -> Result<(), ProgramTraceError> {
    let inconsistent = header.old_rank == 0
        || header.new_rank == 0
        || header.old_image_rank > header.old_rank
        || header.new_image_rank > header.new_rank
        || header.relation_rank == 0
        || header.relation_rank > header.old_image_rank.min(header.new_image_rank)
        || header.relation_rank != header.basis_count;
    if inconsistent {
        return Err(ProgramTraceError::new(
            "correspondence ranks are inconsistent",
        ));
    }
    Ok(())
}

fn decode_correspondence_basis(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    totals: &mut TraceTotals,
) -> Result<Vec<CorrespondenceVector>, ProgramTraceError> {
    let mut basis = Vec::with_capacity(count);
    for _ in 0..count {
        basis.push(decode_correspondence_vector(reader, limits, totals)?);
    }
    Ok(basis)
}

fn decode_correspondence_vector(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    totals: &mut TraceTotals,
) -> Result<CorrespondenceVector, ProgramTraceError> {
    let old_count = reader.usize()?;
    let new_count = reader.usize()?;
    check_correspondence_sides(old_count, new_count)?;
    add_correspondence_terms(totals, old_count, new_count, limits)?;
    Ok(CorrespondenceVector {
        old: decode_correspondence_terms(reader, old_count)?,
        new: decode_correspondence_terms(reader, new_count)?,
    })
}

fn check_correspondence_sides(old_count: usize, new_count: usize) -> Result<(), ProgramTraceError> {
    if old_count == 0 || new_count == 0 {
        return Err(ProgramTraceError::new(
            "a correspondence vector has an empty side",
        ));
    }
    Ok(())
}

fn add_correspondence_terms(
    totals: &mut TraceTotals,
    old_count: usize,
    new_count: usize,
    limits: ProgramTraceDecodeLimits,
) -> Result<(), ProgramTraceError> {
    totals.correspondence_terms = bounded_sum(
        totals.correspondence_terms,
        old_count,
        limits.max_correspondence_terms,
        "correspondence terms",
    )?;
    totals.correspondence_terms = bounded_sum(
        totals.correspondence_terms,
        new_count,
        limits.max_correspondence_terms,
        "correspondence terms",
    )?;
    Ok(())
}

fn decode_correspondence_terms(
    reader: &mut Reader<'_>,
    count: usize,
) -> std::result::Result<Vec<CorrespondenceTerm>, ProgramTraceError> {
    let bytes = count
        .checked_mul(36)
        .ok_or_else(|| ProgramTraceError::new("correspondence term bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(
            "correspondence terms exceed the remaining bytes",
        ));
    }
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        let term = CorrespondenceTerm {
            basis: BasisClassId::from_bytes(reader.array32()?),
            coefficient: reader.u32()?,
        };
        if term.coefficient == 0
            || terms
                .last()
                .is_some_and(|previous: &CorrespondenceTerm| previous.basis >= term.basis)
        {
            return Err(ProgramTraceError::new(
                "correspondence terms are not canonical",
            ));
        }
        terms.push(term);
    }
    Ok(terms)
}

fn encode_diagram(
    out: &mut Vec<u8>,
    diagram: &Diagram,
) -> std::result::Result<(), ProgramTraceError> {
    for bar in &diagram.bars {
        put_usize(out, bar.dim, "bar dimension")?;
        put_u64(out, bar.birth.to_bits());
        put_u64(out, bar.death.to_bits());
    }
    Ok(())
}

fn decode_diagram(
    reader: &mut Reader<'_>,
    count: usize,
) -> std::result::Result<Diagram, ProgramTraceError> {
    let bytes = count
        .checked_mul(24)
        .ok_or_else(|| ProgramTraceError::new("diagram bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(
            "diagram exceeds the remaining bytes",
        ));
    }
    let mut bars = Vec::with_capacity(count);
    for _ in 0..count {
        bars.push(Bar {
            dim: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    let diagram = Diagram { bars };
    let mut canonical = diagram.clone();
    canonical.canonicalize();
    if !diagram_bits_equal(&diagram, &canonical) {
        return Err(ProgramTraceError::new("step bars are not canonical"));
    }
    Ok(diagram)
}

fn mode_tag(mode: ProgramUpdateMode) -> u8 {
    match mode {
        ProgramUpdateMode::Reused => 0,
        ProgramUpdateMode::Repaired => 1,
        ProgramUpdateMode::Recompiled => 2,
    }
}

fn decode_mode(tag: u8) -> std::result::Result<ProgramUpdateMode, ProgramTraceError> {
    match tag {
        0 => Ok(ProgramUpdateMode::Reused),
        1 => Ok(ProgramUpdateMode::Repaired),
        2 => Ok(ProgramUpdateMode::Recompiled),
        _ => Err(ProgramTraceError::new(format!(
            "unknown update-mode tag {tag}"
        ))),
    }
}

fn event_kind_tag(kind: ProgramEventKind) -> u8 {
    match kind {
        ProgramEventKind::VertexSetChanged => 0,
        ProgramEventKind::EdgeSetChanged => 1,
        ProgramEventKind::ThresholdCrossing => 2,
        ProgramEventKind::GuardFailed => 3,
        ProgramEventKind::AtomRebuilt => 4,
        ProgramEventKind::ReductionSuffixRepaired => 5,
        ProgramEventKind::SeparatorContractChanged => 6,
    }
}

fn decode_event_kind(tag: u8) -> std::result::Result<ProgramEventKind, ProgramTraceError> {
    match tag {
        0 => Ok(ProgramEventKind::VertexSetChanged),
        1 => Ok(ProgramEventKind::EdgeSetChanged),
        2 => Ok(ProgramEventKind::ThresholdCrossing),
        3 => Ok(ProgramEventKind::GuardFailed),
        4 => Ok(ProgramEventKind::AtomRebuilt),
        5 => Ok(ProgramEventKind::ReductionSuffixRepaired),
        6 => Ok(ProgramEventKind::SeparatorContractChanged),
        _ => Err(ProgramTraceError::new(format!(
            "unknown event-kind tag {tag}"
        ))),
    }
}

fn continuation_kind_tag(kind: ContinuationKind) -> u8 {
    match kind {
        ContinuationKind::Isomorphism => 0,
        ContinuationKind::Split => 1,
        ContinuationKind::Merge => 2,
        ContinuationKind::Mixing => 3,
        ContinuationKind::Birth => 4,
        ContinuationKind::Death => 5,
        ContinuationKind::Ambiguous => 6,
    }
}

fn decode_continuation_kind(tag: u8) -> std::result::Result<ContinuationKind, ProgramTraceError> {
    match tag {
        0 => Ok(ContinuationKind::Isomorphism),
        1 => Ok(ContinuationKind::Split),
        2 => Ok(ContinuationKind::Merge),
        3 => Ok(ContinuationKind::Mixing),
        4 => Ok(ContinuationKind::Birth),
        5 => Ok(ContinuationKind::Death),
        6 => Ok(ContinuationKind::Ambiguous),
        _ => Err(ProgramTraceError::new(format!(
            "unknown continuation-kind tag {tag}"
        ))),
    }
}

fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> std::result::Result<usize, ProgramTraceError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| ProgramTraceError::new(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(ProgramTraceError::new(format!(
            "{next} {label} exceed the limit {limit}"
        )));
    }
    Ok(next)
}

fn program_artifact_error(error: ProgramArtifactError) -> ProgramTraceError {
    ProgramTraceError::new(error.to_string())
}

fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
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

fn put_usize(
    out: &mut Vec<u8>,
    value: usize,
    label: &str,
) -> std::result::Result<(), ProgramTraceError> {
    let value = u64::try_from(value)
        .map_err(|_| ProgramTraceError::new(format!("{label} does not fit the wire format")))?;
    put_u64(out, value);
    Ok(())
}

fn put_optional_usize(
    out: &mut Vec<u8>,
    value: Option<usize>,
    label: &str,
) -> std::result::Result<(), ProgramTraceError> {
    match value {
        None => out.push(0),
        Some(value) => {
            out.push(1);
            put_usize(out, value, label)?;
        }
    }
    Ok(())
}

fn put_optional_edge(
    out: &mut Vec<u8>,
    edge: Option<EdgeKey>,
) -> std::result::Result<(), ProgramTraceError> {
    match edge {
        None => out.push(0),
        Some(edge) => {
            out.push(1);
            put_usize(out, edge.u, "event edge endpoint")?;
            put_usize(out, edge.v, "event edge endpoint")?;
        }
    }
    Ok(())
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

    fn take(&mut self, count: usize) -> std::result::Result<&'a [u8], ProgramTraceError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProgramTraceError::new("byte position overflows usize"))?;
        if end > self.bytes.len() {
            return Err(ProgramTraceError::new("truncated envelope"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn u8(&mut self) -> std::result::Result<u8, ProgramTraceError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> std::result::Result<u16, ProgramTraceError> {
        Ok(u16::from_be_bytes(
            self.take(2)?.try_into().expect("two-byte slice"),
        ))
    }

    fn u32(&mut self) -> std::result::Result<u32, ProgramTraceError> {
        Ok(u32::from_be_bytes(
            self.take(4)?.try_into().expect("four-byte slice"),
        ))
    }

    fn u64(&mut self) -> std::result::Result<u64, ProgramTraceError> {
        Ok(u64::from_be_bytes(
            self.take(8)?.try_into().expect("eight-byte slice"),
        ))
    }

    fn usize(&mut self) -> std::result::Result<usize, ProgramTraceError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProgramTraceError::new("wire integer does not fit usize"))
    }

    fn bounded_usize(
        &mut self,
        label: &str,
        limit: usize,
    ) -> std::result::Result<usize, ProgramTraceError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProgramTraceError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn optional_usize(&mut self) -> std::result::Result<Option<usize>, ProgramTraceError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.usize()?)),
            tag => Err(ProgramTraceError::new(format!(
                "unknown optional-integer tag {tag}"
            ))),
        }
    }

    fn optional_edge(&mut self) -> std::result::Result<Option<EdgeKey>, ProgramTraceError> {
        match self.u8()? {
            0 => Ok(None),
            1 => {
                let u = self.usize()?;
                let v = self.usize()?;
                if u >= v {
                    return Err(ProgramTraceError::new(
                        "event edge endpoints are not canonical",
                    ));
                }
                Ok(Some(EdgeKey { u, v }))
            }
            tag => Err(ProgramTraceError::new(format!(
                "unknown optional-edge tag {tag}"
            ))),
        }
    }

    fn array32(&mut self) -> std::result::Result<[u8; 32], ProgramTraceError> {
        Ok(self.take(32)?.try_into().expect("32-byte slice"))
    }
}

impl From<ProgramTraceError> for Error {
    fn from(error: ProgramTraceError) -> Self {
        Self::InvalidInput(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn graph(first: f64, second: f64) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, first),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, second),
            ],
        )
        .unwrap()
    }

    #[test]
    fn trace_round_trips_reuse_repair_and_continuation() {
        let initial = graph(4.0, 4.0);
        let updates = [graph(4.01, 4.01), graph(5.0, 4.01), graph(4.0, 4.0)];
        let trace = ProgramTraceArtifact::build(
            &initial,
            &updates,
            &RipsParams::new(1).with_modulus(3),
            CertificateLimits::default(),
        )
        .unwrap();
        assert!(trace.steps.iter().any(|step| {
            step.continuation
                .iter()
                .any(|record| record.kind == ContinuationKind::Split)
        }));
        let bytes = trace.encode().unwrap();
        let decoded = ProgramTraceArtifact::decode(
            &bytes,
            ProgramTraceDecodeLimits::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(decoded.encode().unwrap(), bytes);
        let verified = decoded.verify(CertificateLimits::default()).unwrap();
        assert_eq!(verified.steps.len(), updates.len());
        assert!(diagram_bits_equal(
            &verified.steps.last().unwrap().diagram,
            &verified.final_program.result().diagram
        ));
    }

    #[test]
    fn changed_work_truncation_and_trailing_bytes_are_rejected() {
        let initial = graph(4.0, 4.0);
        let updates = [graph(4.01, 4.01), graph(5.0, 4.01)];
        let trace = ProgramTraceArtifact::build(
            &initial,
            &updates,
            &RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        let bytes = trace.encode().unwrap();
        assert!((0..bytes.len()).all(|end| {
            ProgramTraceArtifact::decode(
                &bytes[..end],
                ProgramTraceDecodeLimits::default(),
                CertificateLimits::default(),
            )
            .is_err()
        }));
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert!(
            ProgramTraceArtifact::decode(
                &trailing,
                ProgramTraceDecodeLimits::default(),
                CertificateLimits::default()
            )
            .is_err()
        );
        let mut changed = trace.clone();
        changed.steps[0].work.edges_checked += 1;
        assert!(changed.verify(CertificateLimits::default()).is_err());
    }

    proptest! {
        #[test]
        fn arbitrary_short_envelopes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..4096)) {
            let _ = ProgramTraceArtifact::decode(
                &bytes,
                ProgramTraceDecodeLimits::default(),
                CertificateLimits::default(),
            );
        }
    }
}
