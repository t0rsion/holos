use std::collections::BTreeSet;

use crate::{ProofError, ProofLimits, is_prime};

use super::model::{CoverageHeader, PhysicalHeader};
use super::source::{canonical_fence, decode_source, validate_radii};
use super::wire::{Reader, decode_indices};
use super::{F64_BITS_CODEC, MAGIC, MODULUS_LIMIT, VERSION};

pub(crate) fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported coverage artifact"));
    }
    Ok(())
}

pub(crate) fn decode_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<CoverageHeader, ProofError> {
    let physical = decode_physical_header(reader, limits)?;
    let failable = decode_indices(reader, physical.vertex_count, limits.max_vertices)?;
    if physical
        .fence
        .iter()
        .any(|vertex| failable.binary_search(vertex).is_ok())
    {
        return Err(ProofError::new("coverage fence vertex is failable"));
    }
    let failure_budget = reader.usize()?;
    if failure_budget > failable.len() {
        return Err(ProofError::new(
            "coverage failure budget exceeds the failable sensor count",
        ));
    }
    let source = decode_source(reader, limits)?;
    Ok(CoverageHeader {
        physical,
        failable,
        failure_budget,
        source,
    })
}

pub(crate) fn decode_physical_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<PhysicalHeader, ProofError> {
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let broadcast_radius = f64::from_bits(reader.u64()?);
    let sensing_radius = f64::from_bits(reader.u64()?);
    validate_radii(broadcast_radius, sensing_radius)?;
    let modulus = reader.u32()?;
    validate_modulus(modulus)?;
    let fence = super::wire::decode_usizes(reader, limits.max_vertices)?;
    validate_fence(vertex_count, &fence)?;
    Ok(PhysicalHeader {
        vertex_count,
        broadcast_radius,
        sensing_radius,
        modulus,
        fence,
    })
}

pub(crate) fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "coverage modulus must be a supported prime",
        ));
    }
    Ok(())
}

pub(crate) fn validate_fence(vertex_count: usize, fence: &[usize]) -> Result<(), ProofError> {
    let distinct = fence.iter().copied().collect::<BTreeSet<_>>().len();
    if fence.len() < 3
        || fence.iter().any(|vertex| *vertex >= vertex_count)
        || distinct != fence.len()
        || canonical_fence(fence) != fence
    {
        return Err(ProofError::new("coverage fence is not a canonical cycle"));
    }
    Ok(())
}
