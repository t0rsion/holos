use crate::proof::ProofError;

use super::claim::ProgramClaim;
use super::model::ProgramProofLimits;
use super::trace_model::{TraceLimits, TraceMode, TraceStep};
use super::trace_wire_records::{
    decode_continuation, decode_correspondence, decode_diagram, decode_event, decode_mode,
    decode_work,
};
use super::wire::{Reader, decode_program, error};
use super::{ProgramEdge, ProgramGraph};

const MAGIC: &[u8; 8] = b"HOLOSDLT";
const VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;
const STEP_MINIMUM_BYTES: usize = 137;
const EVENT_MINIMUM_BYTES: usize = 4;
const CONTINUATION_MINIMUM_BYTES: usize = 25;
const CORRESPONDENCE_MINIMUM_BYTES: usize = 208;

#[derive(Debug, Clone)]
pub(super) struct TraceClaim {
    pub(super) initial_graph: ProgramGraph,
    pub(super) initial_program: ProgramClaim,
    pub(super) steps: Vec<TraceStep>,
}

pub(super) fn decode_trace(
    bytes: &[u8],
    limits: &TraceLimits,
    program_limits: ProgramProofLimits,
) -> Result<TraceClaim, ProofError> {
    if bytes.len() > limits.max_bytes {
        return Err(error(format!(
            "{} bytes exceed the trace limit {}",
            bytes.len(),
            limits.max_bytes
        )));
    }
    let mut reader = Reader::new(bytes);
    let header = decode_trace_header(&mut reader, limits)?;
    let mut total_edges = 0usize;
    let initial_graph = decode_graph(&mut reader, limits, &mut total_edges)?;
    let initial_program = decode_nested_program(
        &mut reader,
        header.initial_program_bytes,
        limits,
        program_limits,
    )?;
    let modulus = initial_program.modulus;
    let mut totals = TraceTotals {
        checkpoint_bytes: header.initial_program_bytes,
        ..TraceTotals::default()
    };
    check_count_bytes(
        &reader,
        header.step_count,
        STEP_MINIMUM_BYTES,
        "trace step records",
    )?;
    let steps = decode_steps(
        &mut reader,
        header.step_count,
        limits,
        program_limits,
        modulus,
        &mut totals,
        &mut total_edges,
    )?;
    if reader.remaining() != 0 {
        return Err(error(format!(
            "{} trailing bytes after the trace envelope",
            reader.remaining()
        )));
    }
    Ok(TraceClaim {
        initial_graph,
        initial_program,
        steps,
    })
}

struct TraceHeader {
    step_count: usize,
    initial_program_bytes: usize,
}

fn decode_trace_header(
    reader: &mut Reader<'_>,
    limits: &TraceLimits,
) -> Result<TraceHeader, ProofError> {
    if reader.take(8)? != MAGIC {
        return Err(error("wrong trace magic bytes"));
    }
    let version = reader.u16()?;
    if version != VERSION {
        return Err(error(format!("unsupported trace wire version {version}")));
    }
    let codec = reader.u8()?;
    if codec != F64_BITS_CODEC {
        return Err(error(format!("unsupported trace scalar codec {codec}")));
    }
    let step_count = reader.bounded_usize("trace step count", limits.max_steps)?;
    let initial_program_bytes =
        reader.bounded_usize("initial program byte count", limits.max_checkpoint_bytes)?;
    Ok(TraceHeader {
        step_count,
        initial_program_bytes,
    })
}

#[derive(Default)]
pub(super) struct TraceTotals {
    pub(super) events: usize,
    pub(super) continuations: usize,
    pub(super) transports: usize,
    pub(super) correspondences: usize,
    pub(super) correspondence_vectors: usize,
    pub(super) correspondence_terms: usize,
    pub(super) bars: usize,
    pub(super) checkpoint_bytes: usize,
}

fn decode_graph(
    reader: &mut Reader<'_>,
    limits: &TraceLimits,
    total_edges: &mut usize,
) -> Result<ProgramGraph, ProofError> {
    let vertices = reader.bounded_usize("graph vertex count", limits.max_vertices)?;
    let edge_count = reader.usize()?;
    *total_edges = bounded_sum(
        *total_edges,
        edge_count,
        limits.max_total_edges,
        "embedded graph edges",
    )?;
    let bytes = edge_count
        .checked_mul(24)
        .ok_or_else(|| error("graph record bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(error("graph record exceeds the remaining bytes"));
    }
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        edges.push(ProgramEdge::new(
            reader.usize()?,
            reader.usize()?,
            f64::from_bits(reader.u64()?),
        ));
    }
    ProgramGraph::from_edges(vertices, edges)
}

fn decode_nested_program(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: &TraceLimits,
    program_limits: ProgramProofLimits,
) -> Result<ProgramClaim, ProofError> {
    if byte_count > limits.max_checkpoint_bytes {
        return Err(error("nested program exceeds the checkpoint limit"));
    }
    let bytes = reader.take(byte_count)?;
    decode_program(
        bytes,
        ProgramProofLimits {
            max_bytes: byte_count.min(program_limits.max_bytes),
            ..program_limits
        },
    )
}

fn decode_steps(
    reader: &mut Reader<'_>,
    count: usize,
    limits: &TraceLimits,
    program_limits: ProgramProofLimits,
    modulus: u32,
    totals: &mut TraceTotals,
    total_edges: &mut usize,
) -> Result<Vec<TraceStep>, ProofError> {
    let mut steps = Vec::with_capacity(count);
    for _ in 0..count {
        steps.push(decode_step(
            reader,
            limits,
            program_limits,
            modulus,
            totals,
            total_edges,
        )?);
    }
    Ok(steps)
}

fn decode_step(
    reader: &mut Reader<'_>,
    limits: &TraceLimits,
    program_limits: ProgramProofLimits,
    modulus: u32,
    totals: &mut TraceTotals,
    total_edges: &mut usize,
) -> Result<TraceStep, ProofError> {
    let header = decode_step_header(reader)?;
    account_step(&header, limits, totals)?;
    let graph = decode_graph(reader, limits, total_edges)?;
    let events = decode_events(reader, header.event_count, graph.vertex_count())?;
    let continuation =
        decode_continuations(reader, header.continuation_count, limits, modulus, totals)?;
    let correspondence =
        decode_correspondences(reader, header.correspondence_count, limits, modulus, totals)?;
    let diagram = decode_diagram(reader, header.bar_count)?;
    let checkpoint = decode_checkpoint(reader, header.checkpoint_bytes, limits, program_limits)?;
    check_step_mode(header.mode, checkpoint.is_some())?;
    Ok(TraceStep {
        graph,
        mode: header.mode,
        work: header.work,
        events,
        continuation,
        correspondence,
        diagram,
        checkpoint,
    })
}

struct StepHeader {
    mode: TraceMode,
    work: super::trace_model::TraceWork,
    event_count: usize,
    continuation_count: usize,
    correspondence_count: usize,
    bar_count: usize,
    checkpoint_bytes: usize,
}

fn decode_step_header(reader: &mut Reader<'_>) -> Result<StepHeader, ProofError> {
    Ok(StepHeader {
        mode: decode_mode(reader.u8()?)?,
        work: decode_work(reader)?,
        event_count: reader.usize()?,
        continuation_count: reader.usize()?,
        correspondence_count: reader.usize()?,
        bar_count: reader.usize()?,
        checkpoint_bytes: reader.usize()?,
    })
}

fn account_step(
    header: &StepHeader,
    limits: &TraceLimits,
    totals: &mut TraceTotals,
) -> Result<(), ProofError> {
    totals.events = bounded_sum(
        totals.events,
        header.event_count,
        limits.max_events,
        "trace events",
    )?;
    totals.continuations = bounded_sum(
        totals.continuations,
        header.continuation_count,
        limits.max_continuations,
        "trace continuations",
    )?;
    totals.correspondences = bounded_sum(
        totals.correspondences,
        header.correspondence_count,
        limits.max_correspondences,
        "trace correspondences",
    )?;
    totals.bars = bounded_sum(totals.bars, header.bar_count, limits.max_bars, "trace bars")?;
    totals.checkpoint_bytes = bounded_sum(
        totals.checkpoint_bytes,
        header.checkpoint_bytes,
        limits.max_checkpoint_bytes,
        "trace checkpoint bytes",
    )?;
    Ok(())
}

fn decode_events(
    reader: &mut Reader<'_>,
    count: usize,
    vertices: usize,
) -> Result<Vec<super::trace_model::TraceEvent>, ProofError> {
    check_count_bytes(reader, count, EVENT_MINIMUM_BYTES, "trace event records")?;
    (0..count).map(|_| decode_event(reader, vertices)).collect()
}

fn decode_continuations(
    reader: &mut Reader<'_>,
    count: usize,
    limits: &TraceLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<Vec<super::trace_model::TraceContinuation>, ProofError> {
    check_count_bytes(
        reader,
        count,
        CONTINUATION_MINIMUM_BYTES,
        "trace continuation records",
    )?;
    (0..count)
        .map(|_| decode_continuation(reader, limits, modulus, totals))
        .collect()
}

fn decode_correspondences(
    reader: &mut Reader<'_>,
    count: usize,
    limits: &TraceLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<Vec<super::trace_model::TraceCorrespondence>, ProofError> {
    check_count_bytes(
        reader,
        count,
        CORRESPONDENCE_MINIMUM_BYTES,
        "trace correspondence records",
    )?;
    (0..count)
        .map(|_| decode_correspondence(reader, limits, modulus, totals))
        .collect()
}

fn decode_checkpoint(
    reader: &mut Reader<'_>,
    bytes: usize,
    limits: &TraceLimits,
    program_limits: ProgramProofLimits,
) -> Result<Option<ProgramClaim>, ProofError> {
    if bytes == 0 {
        Ok(None)
    } else {
        decode_nested_program(reader, bytes, limits, program_limits).map(Some)
    }
}

fn check_step_mode(mode: TraceMode, has_checkpoint: bool) -> Result<(), ProofError> {
    match (mode, has_checkpoint) {
        (TraceMode::Reused, false)
        | (TraceMode::Repaired, true)
        | (TraceMode::Recompiled, true) => Ok(()),
        (TraceMode::Reused, true) => Err(error("reused trace step contains a checkpoint")),
        (_, false) => Err(error("repaired trace step has no checkpoint")),
    }
}

pub(super) fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> Result<usize, ProofError> {
    let next = current
        .checked_add(added)
        .ok_or_else(|| error(format!("{label} overflow usize")))?;
    if next > limit {
        return Err(error(format!("{next} {label} exceed the limit {limit}")));
    }
    Ok(next)
}

fn check_count_bytes(
    reader: &Reader<'_>,
    count: usize,
    minimum: usize,
    label: &str,
) -> Result<(), ProofError> {
    let bytes = count
        .checked_mul(minimum)
        .ok_or_else(|| error(format!("{label} bytes overflow usize")))?;
    if bytes > reader.remaining() {
        return Err(error(format!(
            "{count} {label} exceed the remaining envelope"
        )));
    }
    Ok(())
}

pub(super) fn is_trace(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}
