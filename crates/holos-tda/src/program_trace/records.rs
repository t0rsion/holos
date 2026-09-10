use crate::classes::{BasisClassId, IntervalGroupId};
use crate::{
    Bar, BasisTransport, ClassContinuation, Diagram, EdgeKey, ProgramEvent, ProgramWork,
    ReductionGuardKind, SparseDistanceMatrix,
};

use super::codec::{
    Reader, bounded_sum, continuation_kind_tag, decode_continuation_kind, decode_event_kind,
    diagram_bits_equal, event_kind_tag, put_optional_edge, put_optional_usize, put_u32, put_u64,
    put_usize,
};
use super::model::{ProgramTraceDecodeLimits, ProgramTraceError};

mod correspondence;

pub(crate) use correspondence::{decode_correspondence, encode_correspondence};

pub(crate) fn encode_graph(
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

pub(crate) fn decode_graph(
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

pub(crate) fn encode_work(
    out: &mut Vec<u8>,
    work: ProgramWork,
) -> std::result::Result<(), ProgramTraceError> {
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

pub(crate) fn decode_work(
    reader: &mut Reader<'_>,
) -> std::result::Result<ProgramWork, ProgramTraceError> {
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

pub(crate) fn decode_work_leading(
    reader: &mut Reader<'_>,
) -> Result<[usize; 5], ProgramTraceError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

pub(crate) fn decode_work_reduction(
    reader: &mut Reader<'_>,
) -> Result<[usize; 5], ProgramTraceError> {
    Ok([
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
        reader.usize()?,
    ])
}

pub(crate) fn encode_event(
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

pub(crate) fn decode_event(
    reader: &mut Reader<'_>,
    vertices: usize,
) -> std::result::Result<ProgramEvent, ProgramTraceError> {
    let kind = decode_event_kind(reader.u8()?)?;
    let atom = reader.optional_usize()?;
    let edge = decode_event_edge(reader, vertices)?;
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

fn decode_event_edge(
    reader: &mut Reader<'_>,
    vertices: usize,
) -> std::result::Result<Option<EdgeKey>, ProgramTraceError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => {
            let u = reader.usize()?;
            let v = reader.usize()?;
            if u >= v || v >= vertices {
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

pub(crate) fn encode_continuation(
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

pub(crate) fn decode_continuation(
    reader: &mut Reader<'_>,
    limits: ProgramTraceDecodeLimits,
    modulus: u32,
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
    let transport = decode_basis_transports(reader, transport_count, modulus)?;
    Ok(ClassContinuation {
        kind,
        old_spaces,
        new_spaces,
        transport,
    })
}

pub(crate) fn decode_continuation_counts(
    reader: &mut Reader<'_>,
) -> Result<[usize; 3], ProgramTraceError> {
    Ok([reader.usize()?, reader.usize()?, reader.usize()?])
}

pub(crate) fn check_continuation_bytes(
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

pub(crate) fn decode_interval_group_ids(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<IntervalGroupId>, ProgramTraceError> {
    let mut ids = Vec::with_capacity(count);
    for _ in 0..count {
        ids.push(IntervalGroupId::from_bytes(reader.array32()?));
    }
    Ok(ids)
}

pub(crate) fn decode_basis_transports(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
) -> Result<Vec<BasisTransport>, ProgramTraceError> {
    let mut transports = Vec::with_capacity(count);
    for _ in 0..count {
        let transport = BasisTransport {
            old: BasisClassId::from_bytes(reader.array32()?),
            new: BasisClassId::from_bytes(reader.array32()?),
            coefficient: reader.u32()?,
        };
        if transport.coefficient == 0 || transport.coefficient >= modulus {
            return Err(ProgramTraceError::new(
                "continuation coefficient is outside the field",
            ));
        }
        transports.push(transport);
    }
    Ok(transports)
}

pub(crate) fn encode_diagram(
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

pub(crate) fn decode_diagram(
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
        let bar = Bar {
            dim: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        };
        check_diagram_bar(bar)?;
        bars.push(bar);
    }
    let diagram = Diagram { bars };
    let mut canonical = diagram.clone();
    canonical.canonicalize();
    if !diagram_bits_equal(&diagram, &canonical) {
        return Err(ProgramTraceError::new("step bars are not canonical"));
    }
    Ok(diagram)
}

fn check_diagram_bar(bar: Bar) -> Result<(), ProgramTraceError> {
    if bar.dim > 1 || invalid_birth(bar.birth) || invalid_death(bar.death) {
        return Err(ProgramTraceError::new("diagram contains an invalid bar"));
    }
    if bar.death <= bar.birth {
        return Err(ProgramTraceError::new("diagram contains an invalid bar"));
    }
    Ok(())
}

fn invalid_birth(value: f64) -> bool {
    !value.is_finite() || value < 0.0 || is_negative_zero(value)
}

fn invalid_death(value: f64) -> bool {
    value.is_nan()
        || value < 0.0
        || is_negative_zero(value)
        || (value.is_infinite() && value.is_sign_negative())
}

fn is_negative_zero(value: f64) -> bool {
    value == 0.0 && value.to_bits() != 0
}

#[cfg(test)]
mod tests {
    use super::super::model::TraceTotals;
    use super::*;

    fn put_u32(bytes: &mut Vec<u8>, value: u32) {
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn put_u64(bytes: &mut Vec<u8>, value: u64) {
        bytes.extend_from_slice(&value.to_be_bytes());
    }

    fn put_usize(bytes: &mut Vec<u8>, value: usize) {
        put_u64(bytes, value as u64);
    }

    fn correspondence_bytes(scale: f64, old_coefficient: u32) -> Vec<u8> {
        let mut bytes = vec![0; 64];
        put_u64(&mut bytes, scale.to_bits());
        for _ in 0..6 {
            put_usize(&mut bytes, 1);
        }
        put_usize(&mut bytes, 1);
        put_usize(&mut bytes, 1);
        bytes.extend_from_slice(&[0; 32]);
        put_u32(&mut bytes, old_coefficient);
        bytes.extend_from_slice(&[1; 32]);
        put_u32(&mut bytes, 1);
        bytes
    }

    fn continuation_bytes(coefficient: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.push(0);
        put_usize(&mut bytes, 1);
        put_usize(&mut bytes, 1);
        put_usize(&mut bytes, 1);
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(&[1; 32]);
        bytes.extend_from_slice(&[2; 32]);
        bytes.extend_from_slice(&[3; 32]);
        put_u32(&mut bytes, coefficient);
        bytes
    }

    #[test]
    fn step_diagram_rejects_invalid_scalars_and_bars() {
        for (birth, death) in [
            (f64::NAN, f64::INFINITY),
            (f64::NEG_INFINITY, f64::INFINITY),
            (-0.0, 1.0),
            (0.0, f64::NAN),
            (0.0, -0.0),
            (0.0, f64::NEG_INFINITY),
            (1.0, 1.0),
        ] {
            let mut bytes = Vec::new();
            put_usize(&mut bytes, 0);
            put_u64(&mut bytes, birth.to_bits());
            put_u64(&mut bytes, death.to_bits());
            let mut reader = Reader::new(&bytes);
            let error = decode_diagram(&mut reader, 1).unwrap_err();
            assert_eq!(error.message(), "diagram contains an invalid bar");
        }

        let mut bytes = Vec::new();
        put_usize(&mut bytes, 2);
        put_u64(&mut bytes, 0.0f64.to_bits());
        put_u64(&mut bytes, 1.0f64.to_bits());
        let mut reader = Reader::new(&bytes);
        let error = decode_diagram(&mut reader, 1).unwrap_err();
        assert_eq!(error.message(), "diagram contains an invalid bar");
    }

    #[test]
    fn correspondence_scale_rejects_nan_infinities_and_negative_zero() {
        for scale in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.0, -1.0] {
            let bytes = correspondence_bytes(scale, 1);
            let mut reader = Reader::new(&bytes);
            let mut totals = TraceTotals::default();
            let error = decode_correspondence(
                &mut reader,
                ProgramTraceDecodeLimits::default(),
                3,
                &mut totals,
            )
            .unwrap_err();
            assert_eq!(
                error.message(),
                "correspondence scale must be finite and non-negative"
            );
        }
    }

    #[test]
    fn correspondence_terms_reject_coefficients_outside_the_field() {
        for coefficient in [0, 3, u32::MAX] {
            let bytes = correspondence_bytes(1.0, coefficient);
            let mut reader = Reader::new(&bytes);
            let mut totals = TraceTotals::default();
            let error = decode_correspondence(
                &mut reader,
                ProgramTraceDecodeLimits::default(),
                3,
                &mut totals,
            )
            .unwrap_err();
            assert_eq!(
                error.message(),
                "correspondence coefficient is outside the field"
            );
        }
    }

    #[test]
    fn continuation_transports_reject_coefficients_outside_the_field() {
        for coefficient in [0, 3, u32::MAX] {
            let bytes = continuation_bytes(coefficient);
            let mut reader = Reader::new(&bytes);
            let mut total_transports = 0;
            let error = decode_continuation(
                &mut reader,
                ProgramTraceDecodeLimits::default(),
                3,
                &mut total_transports,
            )
            .unwrap_err();
            assert_eq!(
                error.message(),
                "continuation coefficient is outside the field"
            );
        }
    }

    #[test]
    fn event_edges_reject_endpoints_outside_the_graph() {
        let mut bytes = Vec::new();
        bytes.push(0);
        bytes.push(0);
        bytes.push(1);
        put_usize(&mut bytes, 0);
        put_usize(&mut bytes, 7);
        bytes.push(0);
        let mut reader = Reader::new(&bytes);
        let error = decode_event(&mut reader, 7).unwrap_err();
        assert_eq!(error.message(), "event edge endpoints are not canonical");
    }
}
