use crate::cohomology::Edge;
use crate::{ProofError, ProofLimits};

use super::model::{AffineEdge, Source};
use super::wire_reader::Reader;

pub(super) fn decode_source(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Source, ProofError> {
    match reader.u8()? {
        0 => Ok(Source::Finite),
        1 => decode_affine_source(reader, limits),
        _ => Err(ProofError::new("synthesis source kind is invalid")),
    }
}

fn decode_affine_source(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Source, ProofError> {
    let scenario = reader.u64()?;
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let maximum_rank = reader.usize()?;
    let count = reader.bounded_usize("affine edge count", limits.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(ProofError::new(
            "synthesis affine edge count exceeds the remaining bytes",
        ));
    }
    let mut edges = Vec::with_capacity(count);
    for _ in 0..count {
        edges.push(decode_affine_edge(reader)?);
    }
    Ok(Source::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    })
}

fn decode_affine_edge(reader: &mut Reader<'_>) -> Result<AffineEdge, ProofError> {
    Ok(AffineEdge {
        edge: Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
}
