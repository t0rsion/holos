use crate::proof::{ProofBar, ProofError, check_diagram};

use super::trace_model::{
    TraceBasisTerm, TraceContinuation, TraceContinuationKind, TraceCorrespondence,
    TraceCorrespondenceVector, TraceEvent, TraceEventKind, TraceGuardKind, TraceLimits, TraceMode,
    TraceWork,
};
use super::trace_wire::{TraceTotals, bounded_sum};
use super::wire::{Reader, check_bar, error};

type ContinuationSpaces = (Vec<[u8; 32]>, Vec<[u8; 32]>);
type TraceTransport = ([u8; 32], [u8; 32], u32);

pub(super) fn decode_mode(tag: u8) -> Result<TraceMode, ProofError> {
    match tag {
        0 => Ok(TraceMode::Reused),
        1 => Ok(TraceMode::Repaired),
        2 => Ok(TraceMode::Recompiled),
        _ => Err(error(format!("unknown trace update-mode tag {tag}"))),
    }
}

pub(super) fn decode_work(reader: &mut Reader<'_>) -> Result<TraceWork, ProofError> {
    let first = decode_work_first(reader)?;
    let second = decode_work_second(reader)?;
    Ok(TraceWork {
        edges_checked: first[0],
        h0_edges_scanned: first[1],
        guards_checked: first[2],
        atoms_touched: first[3],
        atoms_reused: first[4],
        atoms_repaired: second[0],
        atoms_rebuilt: second[1],
        reduction_columns_reused: second[2],
        reduction_columns_reduced: second[3],
        reduction_column_additions: second[4],
    })
}

fn decode_work_first(reader: &mut Reader<'_>) -> Result<[usize; 5], ProofError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

fn decode_work_second(reader: &mut Reader<'_>) -> Result<[usize; 5], ProofError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

pub(super) fn decode_event(
    reader: &mut Reader<'_>,
    vertices: usize,
) -> Result<TraceEvent, ProofError> {
    let kind = decode_event_kind(reader)?;
    let atom = decode_optional_usize(reader, "event atom")?;
    let edge = decode_optional_edge(reader, vertices)?;
    let guard = decode_guard_kind(reader)?;
    Ok(TraceEvent {
        kind,
        atom,
        edge,
        guard,
    })
}

fn decode_event_kind(reader: &mut Reader<'_>) -> Result<TraceEventKind, ProofError> {
    match reader.u8()? {
        0 => Ok(TraceEventKind::VertexSetChanged),
        1 => Ok(TraceEventKind::EdgeSetChanged),
        2 => Ok(TraceEventKind::ThresholdCrossing),
        3 => Ok(TraceEventKind::GuardFailed),
        4 => Ok(TraceEventKind::AtomRebuilt),
        5 => Ok(TraceEventKind::ReductionSuffixRepaired),
        6 => Ok(TraceEventKind::SeparatorContractChanged),
        tag => Err(error(format!("unknown trace event-kind tag {tag}"))),
    }
}

fn decode_guard_kind(reader: &mut Reader<'_>) -> Result<Option<TraceGuardKind>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(TraceGuardKind::ChangeOfBasis)),
        2 => Ok(Some(TraceGuardKind::Pivot)),
        tag => Err(error(format!("unknown trace guard-kind tag {tag}"))),
    }
}

pub(super) fn decode_continuation(
    reader: &mut Reader<'_>,
    limits: &TraceLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<TraceContinuation, ProofError> {
    let kind = decode_continuation_kind(reader)?;
    let header = decode_continuation_header(reader, limits, totals)?;
    let (old_spaces, new_spaces) =
        decode_continuation_spaces(reader, header.old_count, header.new_count)?;
    let transport = decode_transports(reader, header.transport_count, modulus)?;
    Ok(TraceContinuation {
        kind,
        old_spaces,
        new_spaces,
        transport,
    })
}

fn decode_continuation_kind(reader: &mut Reader<'_>) -> Result<TraceContinuationKind, ProofError> {
    match reader.u8()? {
        0 => Ok(TraceContinuationKind::Isomorphism),
        1 => Ok(TraceContinuationKind::Split),
        2 => Ok(TraceContinuationKind::Merge),
        3 => Ok(TraceContinuationKind::Mixing),
        4 => Ok(TraceContinuationKind::Birth),
        5 => Ok(TraceContinuationKind::Death),
        6 => Ok(TraceContinuationKind::Ambiguous),
        tag => Err(error(format!("unknown continuation-kind tag {tag}"))),
    }
}

struct ContinuationHeader {
    old_count: usize,
    new_count: usize,
    transport_count: usize,
}

fn decode_continuation_header(
    reader: &mut Reader<'_>,
    limits: &TraceLimits,
    totals: &mut TraceTotals,
) -> Result<ContinuationHeader, ProofError> {
    let old_count = reader.usize()?;
    let new_count = reader.usize()?;
    let transport_count = reader.usize()?;
    totals.transports = bounded_sum(
        totals.transports,
        transport_count,
        limits.max_transports,
        "trace basis transports",
    )?;
    let id_bytes = old_count
        .checked_add(new_count)
        .and_then(|count| count.checked_mul(32))
        .ok_or_else(|| error("continuation id bytes overflow usize"))?;
    let transport_bytes = transport_count
        .checked_mul(68)
        .ok_or_else(|| error("continuation transport bytes overflow usize"))?;
    if id_bytes
        .checked_add(transport_bytes)
        .ok_or_else(|| error("continuation bytes overflow usize"))?
        > reader.remaining()
    {
        return Err(error("continuation exceeds the remaining bytes"));
    }
    Ok(ContinuationHeader {
        old_count,
        new_count,
        transport_count,
    })
}

fn decode_continuation_spaces(
    reader: &mut Reader<'_>,
    old_count: usize,
    new_count: usize,
) -> Result<ContinuationSpaces, ProofError> {
    let old_spaces = (0..old_count)
        .map(|_| reader.array32())
        .collect::<Result<Vec<_>, _>>()?;
    let new_spaces = (0..new_count)
        .map(|_| reader.array32())
        .collect::<Result<Vec<_>, _>>()?;
    Ok((old_spaces, new_spaces))
}

fn decode_transports(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
) -> Result<Vec<TraceTransport>, ProofError> {
    let mut transport = Vec::with_capacity(count);
    for _ in 0..count {
        let old = reader.array32()?;
        let new = reader.array32()?;
        let coefficient = reader.u32()?;
        if coefficient == 0 || coefficient >= modulus {
            return Err(error("continuation coefficient is outside the field"));
        }
        transport.push((old, new, coefficient));
    }
    Ok(transport)
}

pub(super) fn decode_correspondence(
    reader: &mut Reader<'_>,
    limits: &TraceLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<TraceCorrespondence, ProofError> {
    let header = read_correspondence_header(reader)?;
    check_correspondence_header(&header)?;
    check_correspondence_bounds(&header, reader.remaining())?;
    totals.correspondence_vectors = bounded_sum(
        totals.correspondence_vectors,
        header.basis_count,
        limits.max_correspondence_vectors,
        "trace correspondence vectors",
    )?;
    let basis = decode_correspondence_basis(reader, header.basis_count, limits, modulus, totals)?;
    Ok(TraceCorrespondence {
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

struct CorrespondenceHeader {
    old_space: [u8; 32],
    new_space: [u8; 32],
    scale: f64,
    old_rank: usize,
    new_rank: usize,
    old_image_rank: usize,
    new_image_rank: usize,
    relation_rank: usize,
    basis_count: usize,
}

fn read_correspondence_header(reader: &mut Reader<'_>) -> Result<CorrespondenceHeader, ProofError> {
    Ok(CorrespondenceHeader {
        old_space: reader.array32()?,
        new_space: reader.array32()?,
        scale: f64::from_bits(reader.u64()?),
        old_rank: reader.usize()?,
        new_rank: reader.usize()?,
        old_image_rank: reader.usize()?,
        new_image_rank: reader.usize()?,
        relation_rank: reader.usize()?,
        basis_count: reader.usize()?,
    })
}

fn check_correspondence_header(header: &CorrespondenceHeader) -> Result<(), ProofError> {
    check_correspondence_scale(header.scale)?;
    check_correspondence_ranks(header)?;
    Ok(())
}

fn check_correspondence_scale(scale: f64) -> Result<(), ProofError> {
    if !scale.is_finite() || scale < 0.0 || (scale == 0.0 && scale.to_bits() != 0) {
        return Err(error(
            "correspondence scale must be finite and non-negative",
        ));
    }
    Ok(())
}

fn check_correspondence_ranks(header: &CorrespondenceHeader) -> Result<(), ProofError> {
    if header.old_rank == 0
        || header.new_rank == 0
        || header.old_image_rank > header.old_rank
        || header.new_image_rank > header.new_rank
        || header.relation_rank == 0
        || header.relation_rank > header.old_image_rank.min(header.new_image_rank)
        || header.relation_rank != header.basis_count
    {
        return Err(error("correspondence ranks are inconsistent"));
    }
    Ok(())
}

fn check_correspondence_bounds(
    header: &CorrespondenceHeader,
    remaining: usize,
) -> Result<(), ProofError> {
    let minimum_basis_bytes = header
        .basis_count
        .checked_mul(88)
        .ok_or_else(|| error("correspondence basis bytes overflow usize"))?;
    if minimum_basis_bytes > remaining {
        return Err(error("correspondence basis exceeds the envelope"));
    }
    Ok(())
}

fn decode_correspondence_basis(
    reader: &mut Reader<'_>,
    count: usize,
    limits: &TraceLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<Vec<TraceCorrespondenceVector>, ProofError> {
    (0..count)
        .map(|_| decode_correspondence_vector(reader, limits, modulus, totals))
        .collect()
}

fn decode_correspondence_vector(
    reader: &mut Reader<'_>,
    limits: &TraceLimits,
    modulus: u32,
    totals: &mut TraceTotals,
) -> Result<TraceCorrespondenceVector, ProofError> {
    let old_count = reader.usize()?;
    let new_count = reader.usize()?;
    if old_count == 0 || new_count == 0 {
        return Err(error("a correspondence vector has an empty side"));
    }
    totals.correspondence_terms = bounded_sum(
        totals.correspondence_terms,
        old_count,
        limits.max_correspondence_terms,
        "trace correspondence terms",
    )?;
    totals.correspondence_terms = bounded_sum(
        totals.correspondence_terms,
        new_count,
        limits.max_correspondence_terms,
        "trace correspondence terms",
    )?;
    let old = decode_correspondence_terms(reader, old_count, modulus)?;
    let new = decode_correspondence_terms(reader, new_count, modulus)?;
    Ok((old, new))
}

fn decode_correspondence_terms(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
) -> Result<Vec<TraceBasisTerm>, ProofError> {
    let bytes = count
        .checked_mul(36)
        .ok_or_else(|| error("correspondence term bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(error("correspondence terms exceed the envelope"));
    }
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        let basis = reader.array32()?;
        let coefficient = reader.u32()?;
        if coefficient == 0 || coefficient >= modulus {
            return Err(error("correspondence coefficient is outside the field"));
        }
        if terms.last().is_some_and(|(previous, _)| previous >= &basis) {
            return Err(error("correspondence terms are not canonical"));
        }
        terms.push((basis, coefficient));
    }
    Ok(terms)
}

pub(super) fn decode_diagram(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<ProofBar>, ProofError> {
    let bytes = count
        .checked_mul(24)
        .ok_or_else(|| error("trace diagram bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(error("trace diagram exceeds the envelope"));
    }
    let mut diagram = Vec::with_capacity(count);
    for _ in 0..count {
        let bar = ProofBar {
            dimension: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        };
        check_bar(&bar)?;
        diagram.push(bar);
    }
    check_diagram(&diagram)?;
    Ok(diagram)
}

fn decode_optional_usize(
    reader: &mut Reader<'_>,
    label: &str,
) -> Result<Option<usize>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(reader.usize()?)),
        tag => Err(error(format!("unknown {label} option tag {tag}"))),
    }
}

fn decode_optional_edge(
    reader: &mut Reader<'_>,
    vertices: usize,
) -> Result<Option<[usize; 2]>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => {
            let u = reader.usize()?;
            let v = reader.usize()?;
            if u >= v || v >= vertices {
                return Err(error("event edge is not canonical"));
            }
            Ok(Some([u, v]))
        }
        tag => Err(error(format!("unknown event edge option tag {tag}"))),
    }
}
