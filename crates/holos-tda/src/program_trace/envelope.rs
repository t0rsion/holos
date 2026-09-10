use crate::{
    CertificateLimits, ClassContinuation, ClassCorrespondence, ProgramArtifact, ProgramEvent,
};

use super::codec::{
    Reader, bounded_sum, decode_mode, mode_tag, program_artifact_error, put_u16, put_usize,
};
use super::model::{
    DecodedStepBody, ProgramTraceDecodeLimits, ProgramTraceError, ProgramTraceStep, StepCounts,
    TraceHeader, TraceTotals,
};
use super::records::{
    decode_continuation, decode_correspondence, decode_diagram, decode_event, decode_graph,
    decode_work, encode_continuation, encode_correspondence, encode_diagram, encode_event,
    encode_graph, encode_work,
};
use super::replay::check_step_shape;
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};

pub(crate) const STEP_MINIMUM_BYTES: usize = 137;
const EVENT_MINIMUM_BYTES: usize = 4;
const CONTINUATION_MINIMUM_BYTES: usize = 25;
const CORRESPONDENCE_MINIMUM_BYTES: usize = 208;

pub(crate) fn encode_program_artifact(
    artifact: &ProgramArtifact,
) -> std::result::Result<Vec<u8>, ProgramTraceError> {
    artifact.encode().map_err(program_artifact_error)
}

pub(crate) fn encode_checkpoints(
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

pub(crate) fn encode_trace_header(
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

pub(crate) fn encode_trace_step(
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

pub(crate) fn encode_step_header(
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

pub(crate) fn encode_step_body(
    out: &mut Vec<u8>,
    step: &ProgramTraceStep,
) -> std::result::Result<(), ProgramTraceError> {
    encode_events(out, &step.events)?;
    encode_continuations(out, &step.continuation)?;
    encode_correspondences(out, &step.correspondence)?;
    encode_diagram(out, &step.diagram)
}

pub(crate) fn encode_events(
    out: &mut Vec<u8>,
    events: &[ProgramEvent],
) -> std::result::Result<(), ProgramTraceError> {
    for event in events {
        encode_event(out, event)?;
    }
    Ok(())
}

pub(crate) fn encode_continuations(
    out: &mut Vec<u8>,
    continuations: &[ClassContinuation],
) -> std::result::Result<(), ProgramTraceError> {
    for continuation in continuations {
        encode_continuation(out, continuation)?;
    }
    Ok(())
}

pub(crate) fn encode_correspondences(
    out: &mut Vec<u8>,
    correspondences: &[ClassCorrespondence],
) -> std::result::Result<(), ProgramTraceError> {
    for correspondence in correspondences {
        encode_correspondence(out, correspondence)?;
    }
    Ok(())
}

pub(crate) fn check_envelope_size(bytes: &[u8], max_bytes: usize) -> Result<(), ProgramTraceError> {
    if bytes.len() > max_bytes {
        return Err(ProgramTraceError::new(format!(
            "{} bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(())
}

pub(crate) fn decode_trace_header(
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

pub(crate) fn check_trace_identity(reader: &mut Reader<'_>) -> Result<(), ProgramTraceError> {
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

pub(crate) fn decode_program_artifact(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: ProgramTraceDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<ProgramArtifact, ProgramTraceError> {
    ProgramArtifact::decode(reader.take(byte_count)?, limits.program, certificate_limits)
        .map_err(program_artifact_error)
}

pub(crate) fn decode_trace_steps(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    certificate_limits: CertificateLimits,
    modulus: u32,
    totals: &mut TraceTotals,
    total_edges: &mut usize,
) -> Result<Vec<ProgramTraceStep>, ProgramTraceError> {
    let mut steps = Vec::with_capacity(count);
    for _ in 0..count {
        steps.push(decode_trace_step(
            reader,
            limits,
            certificate_limits,
            modulus,
            totals,
            total_edges,
        )?);
    }
    Ok(steps)
}

pub(crate) fn decode_trace_step(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    certificate_limits: CertificateLimits,
    modulus: u32,
    totals: &mut TraceTotals,
    total_edges: &mut usize,
) -> Result<ProgramTraceStep, ProgramTraceError> {
    let mode = decode_mode(reader.u8()?)?;
    let work = decode_work(reader)?;
    let counts = decode_step_counts(reader)?;
    totals.add_step(&counts, limits)?;
    let graph = decode_graph(reader, limits, total_edges)?;
    let body = decode_step_body(reader, &counts, limits, graph.len(), modulus, totals)?;
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

pub(crate) fn decode_step_counts(reader: &mut Reader<'_>) -> Result<StepCounts, ProgramTraceError> {
    Ok(StepCounts {
        events: reader.usize()?,
        continuations: reader.usize()?,
        correspondences: reader.usize()?,
        bars: reader.usize()?,
        checkpoint_bytes: reader.usize()?,
    })
}

pub(crate) fn decode_step_body(
    reader: &mut Reader<'_>,
    counts: &StepCounts,
    limits: ProgramTraceDecodeLimits,
    vertices: usize,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<DecodedStepBody, ProgramTraceError> {
    Ok(DecodedStepBody {
        events: decode_events(reader, counts.events, vertices)?,
        continuation: decode_continuations(reader, counts.continuations, limits, modulus, totals)?,
        correspondence: decode_correspondences(
            reader,
            counts.correspondences,
            limits,
            modulus,
            totals,
        )?,
        diagram: decode_diagram(reader, counts.bars)?,
    })
}

pub(crate) fn decode_events(
    reader: &mut Reader<'_>,
    count: usize,
    vertices: usize,
) -> Result<Vec<ProgramEvent>, ProgramTraceError> {
    check_count_bytes(reader, count, EVENT_MINIMUM_BYTES, "trace event records")?;
    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        events.push(decode_event(reader, vertices)?);
    }
    Ok(events)
}

pub(crate) fn decode_continuations(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<Vec<ClassContinuation>, ProgramTraceError> {
    check_count_bytes(
        reader,
        count,
        CONTINUATION_MINIMUM_BYTES,
        "trace continuation records",
    )?;
    let mut continuations = Vec::with_capacity(count);
    for _ in 0..count {
        continuations.push(decode_continuation(
            reader,
            limits,
            modulus,
            &mut totals.transports,
        )?);
    }
    Ok(continuations)
}

pub(crate) fn decode_correspondences(
    reader: &mut Reader<'_>,
    count: usize,
    limits: ProgramTraceDecodeLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<Vec<ClassCorrespondence>, ProgramTraceError> {
    check_count_bytes(
        reader,
        count,
        CORRESPONDENCE_MINIMUM_BYTES,
        "trace correspondence records",
    )?;
    let mut correspondences = Vec::with_capacity(count);
    for _ in 0..count {
        correspondences.push(decode_correspondence(reader, limits, modulus, totals)?);
    }
    Ok(correspondences)
}

pub(crate) fn decode_checkpoint(
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

pub(crate) fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), ProgramTraceError> {
    if reader.remaining() != 0 {
        return Err(ProgramTraceError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
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

pub(crate) fn check_count_bytes(
    reader: &Reader<'_>,
    count: usize,
    minimum: usize,
    label: &str,
) -> Result<(), ProgramTraceError> {
    let bytes = count
        .checked_mul(minimum)
        .ok_or_else(|| ProgramTraceError::new(format!("{label} bytes overflow usize")))?;
    if bytes > reader.remaining() {
        return Err(ProgramTraceError::new(format!(
            "{count} {label} exceed the remaining envelope"
        )));
    }
    Ok(())
}
