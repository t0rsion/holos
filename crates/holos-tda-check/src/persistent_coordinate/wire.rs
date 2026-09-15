use sha2::{Digest, Sha256};

use crate::cohomology::Edge;
use crate::{CircularProofLimits, ProofError, Reader};

use super::model::DecodedPersistentCoordinate;

pub(super) const MAGIC: &[u8; 8] = b"HOLOSPH\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

pub(super) fn decode(
    bytes: &[u8],
    limits: CircularProofLimits,
) -> Result<DecodedPersistentCoordinate, ProofError> {
    let payload_digest = verify_digest(bytes, limits)?;
    let payload = &bytes[..bytes.len() - 32];
    let mut reader = Reader::new(payload);
    decode_payload(&mut reader, limits, payload_digest)
}

fn decode_payload(
    reader: &mut Reader<'_>,
    limits: CircularProofLimits,
    payload_digest: [u8; 32],
) -> Result<DecodedPersistentCoordinate, ProofError> {
    decode_prefix(reader)?;
    let tolerance = decode_tolerance(reader, limits.max_tolerance)?;
    let nested_bytes = decode_nested(reader, limits)?;
    let (field_multiplier, divisibility) = decode_lift_header(reader)?;
    let integral = decode_integral(reader, limits)?;
    let potential = decode_potential_field(reader, limits)?;
    reject_trailing(reader)?;
    Ok(DecodedPersistentCoordinate {
        nested_bytes,
        tolerance,
        field_multiplier,
        divisibility,
        integral,
        potential,
        payload_digest,
    })
}

fn decode_tolerance(reader: &mut Reader<'_>, maximum: f64) -> Result<f64, ProofError> {
    let tolerance = f64::from_bits(reader.u64()?);
    validate_tolerance(tolerance, maximum)?;
    Ok(tolerance)
}

fn decode_nested(
    reader: &mut Reader<'_>,
    limits: CircularProofLimits,
) -> Result<Vec<u8>, ProofError> {
    let count = reader.bounded_usize(
        "persistent-coordinate nested byte count",
        limits.proof.max_bytes,
    )?;
    let minimum_after_nested = 4 + 8 + 8 + 8;
    if count < 32 || count > reader.remaining() || reader.remaining() - count < minimum_after_nested
    {
        return Err(ProofError::new(
            "persistent-coordinate nested class envelope is truncated",
        ));
    }
    Ok(reader.take(count)?.to_vec())
}

fn decode_lift_header(reader: &mut Reader<'_>) -> Result<(u32, u64), ProofError> {
    let field_multiplier = reader.u32()?;
    let divisibility = reader.u64()?;
    if divisibility == 0 {
        return Err(ProofError::new(
            "persistent-coordinate divisibility is invalid",
        ));
    }
    Ok((field_multiplier, divisibility))
}

fn decode_integral(
    reader: &mut Reader<'_>,
    limits: CircularProofLimits,
) -> Result<Vec<(Edge, i64)>, ProofError> {
    let count = reader.bounded_usize(
        "persistent-coordinate integral term count",
        limits.proof.max_edges.min(limits.proof.max_terms),
    )?;
    if count == 0 {
        return Err(ProofError::new(
            "persistent-coordinate integral lift has no terms",
        ));
    }
    reader.require_bytes(count, 24, "persistent-coordinate integral terms")?;
    decode_integral_terms(reader, count, limits)
}

fn decode_potential_field(
    reader: &mut Reader<'_>,
    limits: CircularProofLimits,
) -> Result<Vec<f64>, ProofError> {
    let count = reader.bounded_usize(
        "persistent-coordinate potential count",
        limits.proof.max_vertices,
    )?;
    reader.require_bytes(count, 8, "persistent-coordinate potential values")?;
    decode_potential(reader, count)
}

fn reject_trailing(reader: &Reader<'_>) -> Result<(), ProofError> {
    if reader.remaining() != 0 {
        return Err(ProofError::new(
            "persistent-coordinate artifact has trailing payload bytes",
        ));
    }
    Ok(())
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new(
            "unsupported persistent-coordinate artifact",
        ));
    }
    Ok(())
}

fn validate_tolerance(tolerance: f64, maximum: f64) -> Result<(), ProofError> {
    if !tolerance.is_finite() || tolerance <= 0.0 || tolerance > maximum {
        return Err(ProofError::new(
            "persistent-coordinate tolerance exceeds the checker limit",
        ));
    }
    Ok(())
}

fn decode_integral_terms(
    reader: &mut Reader<'_>,
    count: usize,
    limits: CircularProofLimits,
) -> Result<Vec<(Edge, i64)>, ProofError> {
    let endpoint_limit = limits.proof.max_vertices.saturating_sub(1);
    let mut terms = Vec::with_capacity(count);
    let mut previous = None;
    for _ in 0..count {
        let u = reader.bounded_usize("persistent-coordinate term endpoint", endpoint_limit)?;
        let v = reader.bounded_usize("persistent-coordinate term endpoint", endpoint_limit)?;
        let coefficient = i64::from_be_bytes(
            reader
                .take(8)?
                .try_into()
                .expect("eight-byte integer coefficient"),
        );
        if u >= v
            || coefficient == 0
            || coefficient.unsigned_abs() > limits.max_integral_coefficient
            || previous.is_some_and(|edge| edge >= (u, v))
        {
            return Err(ProofError::new(
                "persistent-coordinate integral terms are not canonical",
            ));
        }
        previous = Some((u, v));
        terms.push((Edge { u, v }, coefficient));
    }
    Ok(terms)
}

fn decode_potential(reader: &mut Reader<'_>, count: usize) -> Result<Vec<f64>, ProofError> {
    let mut potential = Vec::with_capacity(count);
    for _ in 0..count {
        let value = f64::from_bits(reader.u64()?);
        if !value.is_finite() {
            return Err(ProofError::new(
                "persistent-coordinate potential is not finite",
            ));
        }
        potential.push(value);
    }
    Ok(potential)
}

fn verify_digest(bytes: &[u8], limits: CircularProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() < 32 || bytes.len() > limits.proof.max_bytes {
        return Err(ProofError::new(
            "persistent-coordinate artifact is truncated or exceeds its byte limit",
        ));
    }
    let payload_len = bytes.len() - 32;
    let expected: [u8; 32] = Sha256::digest(&bytes[..payload_len]).into();
    let digest: [u8; 32] = bytes[payload_len..]
        .try_into()
        .expect("32-byte persistent-coordinate digest");
    if digest != expected {
        return Err(ProofError::new(
            "persistent-coordinate digest differs from its content",
        ));
    }
    Ok(digest)
}
