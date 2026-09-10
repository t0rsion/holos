use sha2::{Digest, Sha256};

use crate::cohomology::{Edge, MapTerm};
use crate::{MODULUS_LIMIT, ProofError, ProofLimits, Reader, is_prime};

use super::model::{
    CircularProofLimits, Claim, ClaimHeader, ContinuationClaim, CoordinateCounts,
    CoordinateIdentity, F64_BITS_CODEC, MAGIC, StateClaim, VERSION,
    VerifiedCircularContinuationKind,
};

pub(super) fn decode_claim(bytes: &[u8], limits: CircularProofLimits) -> Result<Claim, ProofError> {
    verify_digest(bytes, limits.proof)?;
    let payload = &bytes[..bytes.len() - 32];
    let mut reader = Reader::new(payload);
    let header = decode_header(&mut reader, limits)?;
    let state_count = decode_state_count(&mut reader, limits)?;
    let mut totals = DecodeTotals::default();
    let states = decode_states(
        &mut reader,
        state_count,
        header.modulus,
        limits,
        &mut totals,
    )?;
    let continuation =
        decode_continuation_presence(&mut reader, header.modulus, limits, &mut totals)?;
    if reader.remaining() != 0 {
        return Err(ProofError::new(
            "circular-coordinate artifact has trailing payload bytes",
        ));
    }
    if (state_count == 2) != continuation.is_some() {
        return Err(ProofError::new(
            "circular continuation does not match the state count",
        ));
    }
    Ok(Claim {
        modulus: header.modulus,
        scale: header.scale,
        tolerance: header.tolerance,
        states,
        continuation,
    })
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: CircularProofLimits,
) -> Result<ClaimHeader, ProofError> {
    decode_prefix(reader)?;
    let modulus = reader.u32()?;
    let scale = f64::from_bits(reader.u64()?);
    let tolerance = f64::from_bits(reader.u64()?);
    validate_modulus(modulus)?;
    validate_scale(scale)?;
    validate_tolerance(tolerance, limits.max_tolerance)?;
    Ok(ClaimHeader {
        modulus,
        scale,
        tolerance,
    })
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported circular-coordinate artifact"));
    }
    Ok(())
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "circular-coordinate modulus is not a supported prime",
        ));
    }
    Ok(())
}

fn validate_scale(scale: f64) -> Result<(), ProofError> {
    if !scale.is_finite() || scale < 0.0 {
        return Err(ProofError::new("circular-coordinate scale is invalid"));
    }
    Ok(())
}

fn validate_tolerance(tolerance: f64, maximum: f64) -> Result<(), ProofError> {
    if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > maximum {
        return Err(ProofError::new(
            "circular-coordinate tolerance exceeds the checker limit",
        ));
    }
    Ok(())
}

fn decode_state_count(
    reader: &mut Reader<'_>,
    limits: CircularProofLimits,
) -> Result<usize, ProofError> {
    let count = reader.bounded_usize("circular state count", limits.proof.max_snapshots.min(2))?;
    if count == 0 {
        return Err(ProofError::new("circular artifact has no state"));
    }
    Ok(count)
}

#[derive(Default)]
struct DecodeTotals {
    edges: usize,
    terms: usize,
}

fn decode_states(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
    limits: CircularProofLimits,
    totals: &mut DecodeTotals,
) -> Result<Vec<StateClaim>, ProofError> {
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
        states.push(decode_state(reader, modulus, limits, totals)?);
    }
    Ok(states)
}

fn decode_state(
    reader: &mut Reader<'_>,
    modulus: u32,
    limits: CircularProofLimits,
    totals: &mut DecodeTotals,
) -> Result<StateClaim, ProofError> {
    let vertex_count = reader.bounded_usize("circular vertex count", limits.proof.max_vertices)?;
    if vertex_count == 0 {
        return Err(ProofError::new("circular state has no vertex"));
    }
    let edges = decode_state_edges(reader, vertex_count, limits, totals)?;
    let coordinate = decode_state_coordinate(reader, vertex_count, modulus, limits, totals)?;
    Ok(StateClaim {
        vertex_count,
        edges,
        coordinate,
    })
}

fn decode_state_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: CircularProofLimits,
    totals: &mut DecodeTotals,
) -> Result<Vec<Edge>, ProofError> {
    let edge_count = reader.bounded_usize("circular edge count", limits.proof.max_edges)?;
    add_edges(totals, edge_count, limits.proof.max_edges)?;
    let mut edges = Vec::with_capacity(edge_count);
    for _ in 0..edge_count {
        edges.push(Edge {
            u: reader.bounded_usize("circular edge endpoint", vertex_count - 1)?,
            v: reader.bounded_usize("circular edge endpoint", vertex_count - 1)?,
        });
    }
    check_edges(&edges, vertex_count)?;
    Ok(edges)
}

fn add_edges(totals: &mut DecodeTotals, count: usize, limit: usize) -> Result<(), ProofError> {
    totals.edges = totals
        .edges
        .checked_add(count)
        .ok_or_else(|| ProofError::new("circular edge count overflows"))?;
    if totals.edges > limit {
        return Err(ProofError::new("circular edges exceed their total limit"));
    }
    Ok(())
}

fn decode_state_coordinate(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    limits: CircularProofLimits,
    totals: &mut DecodeTotals,
) -> Result<Option<super::model::CoordinateClaim>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_coordinate(reader, vertex_count, modulus, limits, totals).map(Some),
        _ => Err(ProofError::new("circular coordinate flag is invalid")),
    }
}

fn decode_coordinate(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    limits: CircularProofLimits,
    totals: &mut DecodeTotals,
) -> Result<super::model::CoordinateClaim, ProofError> {
    let identity = decode_coordinate_identity(reader, modulus)?;
    let counts = decode_coordinate_counts(reader, vertex_count, limits, totals)?;
    let source = decode_source_terms(reader, vertex_count, modulus, counts.source_count)?;
    let integral = decode_integral_terms(
        reader,
        vertex_count,
        limits.max_integral_coefficient,
        counts.integral_count,
    )?;
    let class = decode_class_terms(reader, modulus, limits.proof.max_terms, totals)?;
    if class.is_empty() {
        return Err(ProofError::new("circular class is zero"));
    }
    let potential = decode_potential(reader, counts.potential_count)?;
    Ok(super::model::CoordinateClaim {
        space: identity.space,
        field_multiplier: identity.field_multiplier,
        divisibility: identity.divisibility,
        source,
        integral,
        class,
        potential,
    })
}

fn decode_coordinate_identity(
    reader: &mut Reader<'_>,
    modulus: u32,
) -> Result<CoordinateIdentity, ProofError> {
    let mut space = [0u8; 32];
    space.copy_from_slice(reader.take(32)?);
    let field_multiplier = reader.u32()?;
    let divisibility = reader.u64()?;
    if field_multiplier == 0 || field_multiplier >= modulus || divisibility == 0 {
        return Err(ProofError::new(
            "circular lift multiplier or divisibility is invalid",
        ));
    }
    Ok(CoordinateIdentity {
        space,
        field_multiplier,
        divisibility,
    })
}

fn decode_coordinate_counts(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: CircularProofLimits,
    totals: &mut DecodeTotals,
) -> Result<CoordinateCounts, ProofError> {
    let source_count =
        reader.bounded_usize("circular source term count", limits.proof.max_terms)?;
    let integral_count =
        reader.bounded_usize("circular integral term count", limits.proof.max_terms)?;
    let potential_count =
        reader.bounded_usize("circular potential count", limits.proof.max_vertices)?;
    if potential_count != vertex_count || source_count == 0 || integral_count == 0 {
        return Err(ProofError::new(
            "circular coordinate collection counts are invalid",
        ));
    }
    add_terms(totals, source_count, limits.proof.max_terms)?;
    add_terms(totals, integral_count, limits.proof.max_terms)?;
    Ok(CoordinateCounts {
        source_count,
        integral_count,
        potential_count,
    })
}

fn decode_source_terms(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    modulus: u32,
    count: usize,
) -> Result<Vec<(Edge, u32)>, ProofError> {
    let mut source = Vec::with_capacity(count);
    for _ in 0..count {
        let edge = decode_term_edge(reader, vertex_count)?;
        let coefficient = reader.u32()?;
        if coefficient == 0 || coefficient >= modulus {
            return Err(ProofError::new("circular field coefficient is invalid"));
        }
        source.push((edge, coefficient));
    }
    check_field_terms(&source)?;
    Ok(source)
}

fn decode_integral_terms(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum_coefficient: u64,
    count: usize,
) -> Result<Vec<(Edge, i64)>, ProofError> {
    let mut integral = Vec::with_capacity(count);
    for _ in 0..count {
        let edge = decode_term_edge(reader, vertex_count)?;
        let bytes = reader.take(8)?;
        let coefficient = i64::from_be_bytes(bytes.try_into().expect("eight bytes"));
        if coefficient == 0 || coefficient.unsigned_abs() > maximum_coefficient {
            return Err(ProofError::new(
                "circular integer coefficient exceeds its limit",
            ));
        }
        integral.push((edge, coefficient));
    }
    check_integral_terms(&integral)?;
    Ok(integral)
}

fn decode_potential(reader: &mut Reader<'_>, count: usize) -> Result<Vec<f64>, ProofError> {
    let mut potential = Vec::with_capacity(count);
    for _ in 0..count {
        let value = f64::from_bits(reader.u64()?);
        if !value.is_finite() {
            return Err(ProofError::new("circular potential is not finite"));
        }
        potential.push(value);
    }
    Ok(potential)
}

fn decode_term_edge(reader: &mut Reader<'_>, vertex_count: usize) -> Result<Edge, ProofError> {
    Ok(Edge {
        u: reader.bounded_usize("circular term endpoint", vertex_count - 1)?,
        v: reader.bounded_usize("circular term endpoint", vertex_count - 1)?,
    })
}

fn decode_continuation_presence(
    reader: &mut Reader<'_>,
    modulus: u32,
    limits: CircularProofLimits,
    totals: &mut DecodeTotals,
) -> Result<Option<ContinuationClaim>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_continuation(reader, modulus, limits.proof.max_terms, totals).map(Some),
        _ => Err(ProofError::new(
            "circular continuation presence flag is invalid",
        )),
    }
}

fn decode_continuation(
    reader: &mut Reader<'_>,
    modulus: u32,
    term_limit: usize,
    totals: &mut DecodeTotals,
) -> Result<ContinuationClaim, ProofError> {
    let kind = decode_continuation_kind(reader)?;
    let target = decode_class_terms(reader, modulus, term_limit, totals)?;
    let ambiguity = decode_ambiguity(reader, modulus, term_limit, totals)?;
    Ok(ContinuationClaim {
        kind,
        target,
        ambiguity,
    })
}

fn decode_continuation_kind(
    reader: &mut Reader<'_>,
) -> Result<VerifiedCircularContinuationKind, ProofError> {
    match reader.u8()? {
        1 => Ok(VerifiedCircularContinuationKind::Unique),
        2 => Ok(VerifiedCircularContinuationKind::Ambiguous),
        3 => Ok(VerifiedCircularContinuationKind::NoExtension),
        4 => Ok(VerifiedCircularContinuationKind::NoNonzeroContinuation),
        _ => Err(ProofError::new("circular continuation kind is invalid")),
    }
}

fn decode_ambiguity(
    reader: &mut Reader<'_>,
    modulus: u32,
    term_limit: usize,
    totals: &mut DecodeTotals,
) -> Result<Vec<Vec<MapTerm>>, ProofError> {
    let row_count = reader.bounded_usize("circular ambiguity row count", term_limit)?;
    let mut ambiguity = Vec::with_capacity(row_count);
    for _ in 0..row_count {
        ambiguity.push(decode_class_terms(reader, modulus, term_limit, totals)?);
    }
    Ok(ambiguity)
}

fn decode_class_terms(
    reader: &mut Reader<'_>,
    modulus: u32,
    term_limit: usize,
    totals: &mut DecodeTotals,
) -> Result<Vec<MapTerm>, ProofError> {
    let count = reader.bounded_usize("circular class term count", term_limit)?;
    add_terms(totals, count, term_limit)?;
    let mut terms = Vec::with_capacity(count);
    for _ in 0..count {
        let target = reader.bounded_usize("circular class basis index", term_limit)?;
        let coefficient = reader.u32()?;
        if coefficient == 0 || coefficient >= modulus {
            return Err(ProofError::new("circular class coefficient is invalid"));
        }
        terms.push(MapTerm {
            target,
            coefficient,
        });
    }
    if terms
        .windows(2)
        .any(|pair| pair[0].target >= pair[1].target)
    {
        return Err(ProofError::new("circular class terms are not canonical"));
    }
    Ok(terms)
}

fn add_terms(totals: &mut DecodeTotals, count: usize, limit: usize) -> Result<(), ProofError> {
    totals.terms = totals
        .terms
        .checked_add(count)
        .ok_or_else(|| ProofError::new("circular term count overflows"))?;
    if totals.terms > limit {
        return Err(ProofError::new("circular terms exceed their total limit"));
    }
    Ok(())
}

fn check_edges(edges: &[Edge], vertex_count: usize) -> Result<(), ProofError> {
    if edges
        .iter()
        .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new("circular active edges are not canonical"));
    }
    Ok(())
}

fn check_field_terms(terms: &[(Edge, u32)]) -> Result<(), ProofError> {
    if terms.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(ProofError::new("circular source terms are not canonical"));
    }
    Ok(())
}

fn check_integral_terms(terms: &[(Edge, i64)]) -> Result<(), ProofError> {
    if terms.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
        return Err(ProofError::new("circular integral terms are not canonical"));
    }
    Ok(())
}

fn verify_digest(bytes: &[u8], limits: ProofLimits) -> Result<(), ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "circular artifact exceeds its byte limit or is truncated",
        ));
    }
    let payload_len = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-circular-coordinate-v1");
    hash.update(&bytes[..payload_len]);
    let expected: [u8; 32] = hash.finalize().into();
    if bytes[payload_len..] != expected {
        return Err(ProofError::new(
            "circular artifact digest differs from its content",
        ));
    }
    Ok(())
}
