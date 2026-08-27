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
        let initial_program = self
            .initial_program
            .encode()
            .map_err(program_artifact_error)?;
        let checkpoints = self
            .steps
            .iter()
            .map(|step| {
                step.checkpoint
                    .as_ref()
                    .map(ProgramArtifact::encode)
                    .transpose()
                    .map_err(program_artifact_error)
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut out = Vec::new();
        out.extend_from_slice(MAGIC);
        put_u16(&mut out, WIRE_VERSION);
        out.push(F64_BITS_CODEC);
        put_usize(&mut out, self.steps.len(), "step count")?;
        put_usize(
            &mut out,
            initial_program.len(),
            "initial program byte count",
        )?;
        encode_graph(&mut out, &self.initial_graph)?;
        out.extend_from_slice(&initial_program);
        for (step, checkpoint) in self.steps.iter().zip(checkpoints) {
            out.push(mode_tag(step.mode));
            encode_work(&mut out, step.work)?;
            put_usize(&mut out, step.events.len(), "event count")?;
            put_usize(&mut out, step.continuation.len(), "continuation count")?;
            put_usize(&mut out, step.correspondence.len(), "correspondence count")?;
            put_usize(&mut out, step.diagram.bars.len(), "bar count")?;
            put_usize(
                &mut out,
                checkpoint.as_ref().map_or(0, Vec::len),
                "checkpoint byte count",
            )?;
            encode_graph(&mut out, &step.graph)?;
            for event in &step.events {
                encode_event(&mut out, event)?;
            }
            for continuation in &step.continuation {
                encode_continuation(&mut out, continuation)?;
            }
            for correspondence in &step.correspondence {
                encode_correspondence(&mut out, correspondence)?;
            }
            encode_diagram(&mut out, &step.diagram)?;
            if let Some(checkpoint) = checkpoint {
                out.extend_from_slice(&checkpoint);
            }
        }
        Ok(out)
    }

    /// Decode and structurally validate a bounded program trace.
    pub fn decode(
        bytes: &[u8],
        limits: ProgramTraceDecodeLimits,
        certificate_limits: CertificateLimits,
    ) -> std::result::Result<Self, ProgramTraceError> {
        if bytes.len() > limits.max_bytes {
            return Err(ProgramTraceError::new(format!(
                "{} bytes exceed the decoder limit {}",
                bytes.len(),
                limits.max_bytes
            )));
        }
        let mut reader = Reader::new(bytes);
        if reader.take(8)? != MAGIC {
            return Err(ProgramTraceError::new("wrong magic bytes"));
        }
        if reader.u16()? != WIRE_VERSION {
            return Err(ProgramTraceError::new("unsupported wire version"));
        }
        if reader.u8()? != F64_BITS_CODEC {
            return Err(ProgramTraceError::new("unsupported scalar codec"));
        }
        let step_count = reader.bounded_usize("step count", limits.max_steps)?;
        let initial_program_bytes =
            reader.bounded_usize("initial program byte count", limits.max_checkpoint_bytes)?;
        let mut total_edges = 0usize;
        let initial_graph = decode_graph(&mut reader, limits, &mut total_edges)?;
        let initial_program = ProgramArtifact::decode(
            reader.take(initial_program_bytes)?,
            limits.program,
            certificate_limits,
        )
        .map_err(program_artifact_error)?;
        let mut totals = TraceTotals {
            checkpoint_bytes: initial_program_bytes,
            ..TraceTotals::default()
        };
        let mut steps = Vec::with_capacity(step_count);
        for _ in 0..step_count {
            let mode = decode_mode(reader.u8()?)?;
            let work = decode_work(&mut reader)?;
            let event_count = reader.usize()?;
            let continuation_count = reader.usize()?;
            let correspondence_count = reader.usize()?;
            let bar_count = reader.usize()?;
            let checkpoint_bytes = reader.usize()?;
            totals.events = bounded_sum(totals.events, event_count, limits.max_events, "events")?;
            totals.continuations = bounded_sum(
                totals.continuations,
                continuation_count,
                limits.max_continuations,
                "continuations",
            )?;
            totals.correspondences = bounded_sum(
                totals.correspondences,
                correspondence_count,
                limits.max_correspondences,
                "correspondences",
            )?;
            totals.bars = bounded_sum(totals.bars, bar_count, limits.max_bars, "bars")?;
            totals.checkpoint_bytes = bounded_sum(
                totals.checkpoint_bytes,
                checkpoint_bytes,
                limits.max_checkpoint_bytes,
                "checkpoint bytes",
            )?;
            let graph = decode_graph(&mut reader, limits, &mut total_edges)?;
            let mut events = Vec::with_capacity(event_count);
            for _ in 0..event_count {
                events.push(decode_event(&mut reader)?);
            }
            let mut continuation = Vec::with_capacity(continuation_count);
            for _ in 0..continuation_count {
                continuation.push(decode_continuation(
                    &mut reader,
                    limits,
                    &mut totals.transports,
                )?);
            }
            let mut correspondence = Vec::with_capacity(correspondence_count);
            for _ in 0..correspondence_count {
                correspondence.push(decode_correspondence(&mut reader, limits, &mut totals)?);
            }
            let diagram = decode_diagram(&mut reader, bar_count)?;
            let checkpoint = if checkpoint_bytes == 0 {
                None
            } else {
                Some(
                    ProgramArtifact::decode(
                        reader.take(checkpoint_bytes)?,
                        limits.program,
                        certificate_limits,
                    )
                    .map_err(program_artifact_error)?,
                )
            };
            let step = ProgramTraceStep {
                graph,
                mode,
                work,
                events,
                continuation,
                correspondence,
                diagram,
                checkpoint,
            };
            check_step_shape(&step)?;
            steps.push(step);
        }
        if reader.remaining() != 0 {
            return Err(ProgramTraceError::new(format!(
                "{} trailing bytes after the envelope",
                reader.remaining()
            )));
        }
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
        let mut program = self
            .initial_program
            .verify(&self.initial_graph, certificate_limits)
            .map_err(program_artifact_error)?;
        let initial_result = program.result().clone();
        let mut verified_steps = Vec::with_capacity(self.steps.len());
        for (index, step) in self.steps.iter().enumerate() {
            check_step_shape(step)?;
            let checked = if step.mode == ProgramUpdateMode::Reused {
                program
                    .advance_reused(&step.graph)
                    .map_err(|error| ProgramTraceError::new(error.to_string()))?
            } else {
                let checkpoint = step.checkpoint.as_ref().ok_or_else(|| {
                    ProgramTraceError::new(format!("step {index} has no required checkpoint"))
                })?;
                let replacement = checkpoint
                    .verify(&step.graph, certificate_limits)
                    .map_err(program_artifact_error)?;
                let (mode, events, work) = program
                    .preview_update(&step.graph, replacement.states().len())
                    .map_err(|error| ProgramTraceError::new(error.to_string()))?;
                let continuation =
                    class_continuation(&program.result().spaces, &replacement.result().spaces);
                let correspondence = crate::class_correspondences(
                    program.current_graph(),
                    &program.result().spaces,
                    &step.graph,
                    &replacement.result().spaces,
                    replacement.params().modulus,
                )
                .map_err(|error| ProgramTraceError::new(error.to_string()))?;
                let result = replacement.result().clone();
                program = replacement;
                crate::ProgramUpdate {
                    result,
                    mode,
                    events,
                    continuation,
                    correspondence,
                    work,
                }
            };
            if checked.mode != step.mode
                || checked.work != step.work
                || checked.events != step.events
                || checked.continuation != step.continuation
                || checked.correspondence != step.correspondence
                || !diagram_bits_equal(&checked.result.diagram, &step.diagram)
            {
                return Err(ProgramTraceError::new(format!(
                    "step {index} differs from independent replay"
                )));
            }
            verified_steps.push(VerifiedProgramTraceStep {
                mode: checked.mode,
                work: checked.work,
                events: checked.events,
                continuation: checked.continuation,
                correspondence: checked.correspondence,
                diagram: checked.result.diagram,
            });
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
    Ok(ProgramWork {
        edges_checked: reader.usize()?,
        h0_edges_scanned: reader.usize()?,
        guards_checked: reader.usize()?,
        atoms_touched: reader.usize()?,
        atoms_reused: reader.usize()?,
        atoms_repaired: reader.usize()?,
        atoms_rebuilt: reader.usize()?,
        reduction_columns_reused: reader.usize()?,
        reduction_columns_reduced: reader.usize()?,
        reduction_column_additions: reader.usize()?,
    })
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
    let old_count = reader.usize()?;
    let new_count = reader.usize()?;
    let transport_count = reader.usize()?;
    *total_transports = bounded_sum(
        *total_transports,
        transport_count,
        limits.max_transports,
        "basis transports",
    )?;
    let bytes = old_count
        .checked_add(new_count)
        .and_then(|ids| ids.checked_mul(32))
        .and_then(|ids| {
            transport_count
                .checked_mul(68)
                .and_then(|terms| ids.checked_add(terms))
        })
        .ok_or_else(|| ProgramTraceError::new("continuation bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(
            "continuation exceeds the remaining bytes",
        ));
    }
    let mut old_spaces = Vec::with_capacity(old_count);
    for _ in 0..old_count {
        old_spaces.push(IntervalGroupId::from_bytes(reader.array32()?));
    }
    let mut new_spaces = Vec::with_capacity(new_count);
    for _ in 0..new_count {
        new_spaces.push(IntervalGroupId::from_bytes(reader.array32()?));
    }
    let mut transport = Vec::with_capacity(transport_count);
    for _ in 0..transport_count {
        transport.push(BasisTransport {
            old: BasisClassId::from_bytes(reader.array32()?),
            new: BasisClassId::from_bytes(reader.array32()?),
            coefficient: reader.u32()?,
        });
    }
    Ok(ClassContinuation {
        kind,
        old_spaces,
        new_spaces,
        transport,
    })
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
    let old_space = IntervalGroupId::from_bytes(reader.array32()?);
    let new_space = IntervalGroupId::from_bytes(reader.array32()?);
    let scale = f64::from_bits(reader.u64()?);
    if !scale.is_finite() || scale < 0.0 {
        return Err(ProgramTraceError::new(
            "correspondence scale must be finite and non-negative",
        ));
    }
    let old_rank = reader.usize()?;
    let new_rank = reader.usize()?;
    let old_image_rank = reader.usize()?;
    let new_image_rank = reader.usize()?;
    let relation_rank = reader.usize()?;
    let basis_count = reader.usize()?;
    if old_rank == 0
        || new_rank == 0
        || old_image_rank > old_rank
        || new_image_rank > new_rank
        || relation_rank == 0
        || relation_rank > old_image_rank.min(new_image_rank)
        || relation_rank != basis_count
    {
        return Err(ProgramTraceError::new(
            "correspondence ranks are inconsistent",
        ));
    }
    totals.correspondence_vectors = bounded_sum(
        totals.correspondence_vectors,
        basis_count,
        limits.max_correspondence_vectors,
        "correspondence vectors",
    )?;
    let mut basis = Vec::with_capacity(basis_count);
    for _ in 0..basis_count {
        let old_count = reader.usize()?;
        let new_count = reader.usize()?;
        if old_count == 0 || new_count == 0 {
            return Err(ProgramTraceError::new(
                "a correspondence vector has an empty side",
            ));
        }
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
        let old = decode_correspondence_terms(reader, old_count)?;
        let new = decode_correspondence_terms(reader, new_count)?;
        basis.push(CorrespondenceVector { old, new });
    }
    Ok(ClassCorrespondence {
        old_space,
        new_space,
        scale,
        old_rank,
        new_rank,
        old_image_rank,
        new_image_rank,
        relation_rank,
        basis,
    })
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
