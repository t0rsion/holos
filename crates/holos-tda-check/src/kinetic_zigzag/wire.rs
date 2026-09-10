use sha2::{Digest, Sha256};

use crate::{MODULUS_LIMIT, ProofError, ProofLimits, Reader, is_prime};

use super::model::{AffineEdge, Interval, KineticClaim, KineticHeader, KineticOutput};

pub(super) const MAGIC: &[u8; 8] = b"HOLOSZZ\0";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;

pub(super) fn decode_claim(bytes: &[u8], limits: ProofLimits) -> Result<KineticClaim, ProofError> {
    let expected = expected_digest(bytes, limits)?;
    let mut reader = Reader::new(bytes);
    decode_prefix(&mut reader)?;
    let vertex_count = reader.bounded_usize("zigzag vertex count", limits.max_vertices)?;
    let trajectories = decode_trajectories(&mut reader, limits)?;
    let claim = decode_claim_body(&mut reader, vertex_count, trajectories, limits)?;
    decode_trailer(&mut reader, expected)?;
    Ok(claim)
}

fn expected_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "kinetic zigzag exceeds its byte limit or is truncated",
        ));
    }
    let payload_len = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-kinetic-zigzag-v1");
    hash.update(&bytes[..payload_len]);
    let expected = hash.finalize().into();
    if bytes[payload_len..] != expected {
        return Err(ProofError::new(
            "kinetic zigzag digest differs from its content",
        ));
    }
    Ok(expected)
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(ProofError::new("unsupported kinetic zigzag artifact"))
    } else {
        Ok(())
    }
}

fn decode_trajectories(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Vec<AffineEdge>, ProofError> {
    let count = reader.bounded_usize("zigzag edge count", limits.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(ProofError::new(
            "kinetic zigzag edge count exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| decode_trajectory(reader)).collect()
}

fn decode_trajectory(reader: &mut Reader<'_>) -> Result<AffineEdge, ProofError> {
    Ok(AffineEdge {
        edge: crate::cohomology::Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
}

fn decode_claim_body(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    trajectories: Vec<AffineEdge>,
    limits: ProofLimits,
) -> Result<KineticClaim, ProofError> {
    let header = decode_kinetic_header(reader, vertex_count, &trajectories, limits)?;
    let output = decode_kinetic_output(reader, limits)?;
    Ok(KineticClaim {
        vertex_count,
        trajectories,
        start: header.start,
        end: header.end,
        dimension: header.dimension,
        scale: header.scale,
        modulus: header.modulus,
        persistent_ties: header.persistent_ties,
        node_ranks: output.node_ranks,
        node_edges: output.node_edges,
        arrow_ranks: output.arrow_ranks,
        generalized_ranks: output.generalized_ranks,
        intervals: output.intervals,
    })
}

fn decode_kinetic_header(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    trajectories: &[AffineEdge],
    limits: ProofLimits,
) -> Result<KineticHeader, ProofError> {
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let dimension = reader.bounded_usize("zigzag dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    let persistent_ties = reader.usize()?;
    validate_input(vertex_count, trajectories, start, end, scale, modulus)?;
    Ok(KineticHeader {
        start,
        end,
        dimension,
        scale,
        modulus,
        persistent_ties,
    })
}

fn decode_kinetic_output(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<KineticOutput, ProofError> {
    let node_ranks = decode_node_ranks(reader, limits)?;
    let node_edges = read_usizes(reader, "zigzag node edge count", limits.max_snapshots)?;
    let arrow_ranks = read_usizes(reader, "zigzag arrow rank", limits.max_references)?;
    let maximum_ranks = square_count(node_ranks.len())?;
    let generalized_ranks = read_usizes(reader, "zigzag generalized rank", maximum_ranks)?;
    let intervals = decode_intervals(reader, node_ranks.len())?;
    Ok(KineticOutput {
        node_ranks,
        node_edges,
        arrow_ranks,
        generalized_ranks,
        intervals,
    })
}

fn decode_node_ranks(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Vec<usize>, ProofError> {
    let ranks = read_usizes(reader, "zigzag node rank", limits.max_snapshots)?;
    if ranks.is_empty() || ranks.len() > super::model::FORMAT_MAX_NODES {
        Err(ProofError::new(
            "kinetic zigzag node count exceeds the format limit",
        ))
    } else {
        Ok(ranks)
    }
}

fn square_count(count: usize) -> Result<usize, ProofError> {
    count
        .checked_mul(count)
        .ok_or_else(|| ProofError::new("kinetic zigzag rank count overflows"))
}

fn decode_intervals(
    reader: &mut Reader<'_>,
    node_count: usize,
) -> Result<Vec<Interval>, ProofError> {
    let maximum = node_count
        .checked_mul(node_count.saturating_add(1))
        .map(|value| value / 2)
        .ok_or_else(|| ProofError::new("kinetic zigzag interval count overflows"))?;
    let count = reader.bounded_usize("zigzag interval count", maximum)?;
    (0..count)
        .map(|_| Ok((reader.usize()?, reader.usize()?, reader.usize()?)))
        .collect()
}

fn decode_trailer(reader: &mut Reader<'_>, expected: [u8; 32]) -> Result<(), ProofError> {
    if reader.array32()? != expected || reader.remaining() != 0 {
        Err(ProofError::new(
            "kinetic zigzag has a wrong digest or trailing bytes",
        ))
    } else {
        Ok(())
    }
}

fn validate_input(
    vertex_count: usize,
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    modulus: u32,
) -> Result<(), ProofError> {
    validate_interval(start, end, scale)?;
    validate_modulus(modulus)?;
    let mut previous = None;
    for trajectory in edges {
        validate_trajectory(trajectory, previous, vertex_count, start, end)?;
        previous = Some(trajectory.edge);
    }
    Ok(())
}

fn validate_interval(start: f64, end: f64, scale: f64) -> Result<(), ProofError> {
    if !start.is_finite() || !end.is_finite() || start >= end || !scale.is_finite() || scale < 0.0 {
        Err(ProofError::new("kinetic zigzag interval is invalid"))
    } else {
        Ok(())
    }
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(modulus as u64) || u64::from(modulus) >= MODULUS_LIMIT {
        Err(ProofError::new(
            "kinetic zigzag modulus is not a supported prime",
        ))
    } else {
        Ok(())
    }
}

fn validate_trajectory(
    trajectory: &AffineEdge,
    previous: Option<crate::cohomology::Edge>,
    vertex_count: usize,
    start: f64,
    end: f64,
) -> Result<(), ProofError> {
    if trajectory.edge.u >= trajectory.edge.v
        || trajectory.edge.v >= vertex_count
        || !trajectory.intercept.is_finite()
        || !trajectory.velocity.is_finite()
        || previous.is_some_and(|edge| edge >= trajectory.edge)
    {
        return Err(ProofError::new(
            "kinetic zigzag edge trajectory is not canonical",
        ));
    }
    validate_trajectory_weight(trajectory, start)?;
    validate_trajectory_weight(trajectory, end)
}

fn validate_trajectory_weight(trajectory: &AffineEdge, time: f64) -> Result<(), ProofError> {
    let weight = trajectory.intercept + trajectory.velocity * time;
    if !weight.is_finite() || weight < 0.0 {
        Err(ProofError::new(
            "kinetic zigzag edge weight leaves its valid range",
        ))
    } else {
        Ok(())
    }
}

fn read_usizes(
    reader: &mut Reader<'_>,
    name: &str,
    maximum: usize,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize(name, maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(format!(
            "{name} count exceeds the remaining bytes"
        )));
    }
    (0..count).map(|_| reader.usize()).collect()
}
