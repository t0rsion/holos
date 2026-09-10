//! Canonical coverage artifact decoding.

mod proof;
mod reader;

use crate::{
    CoverageFence, CoverageSynthesisLimits, CoverageSynthesisStatus, Error, KineticEdge,
    PlanarCoverageModel, Result,
};

use super::super::model::{
    CoverageAction, CoverageHeader, CoverageProducerWork, CoverageSelectionData, CoverageSource,
    CoverageSpecification, CoverageState, CoverageWorkLimits, DecodedCoverageSearch,
};
use super::{F64_BITS_CODEC, MAGIC, VERSION};
use reader::{decode_edges, decode_indices, decode_usizes};

pub(super) use proof::decode_coverage_proof_data;
pub(super) use reader::Reader;

pub(super) fn decode_coverage_prefix(reader: &mut Reader<'_>) -> Result<()> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        Err(Error::InvalidInput("unsupported coverage artifact".into()))
    } else {
        Ok(())
    }
}

pub(super) fn decode_coverage_specification(
    reader: &mut Reader<'_>,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageSpecification> {
    let header = decode_coverage_header(reader, limits)?;
    let states = decode_coverage_states(reader, header.vertex_count, limits)?;
    Ok(CoverageSpecification {
        vertex_count: header.vertex_count,
        model: header.model,
        modulus: header.modulus,
        fence: header.fence,
        failable_vertices: header.failable_vertices,
        failure_budget: header.failure_budget,
        source: header.source,
        states,
    })
}

fn decode_coverage_header(
    reader: &mut Reader<'_>,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageHeader> {
    let vertex_count = reader.bounded_usize("vertex count", limits.coverage.max_vertices)?;
    Ok(CoverageHeader {
        vertex_count,
        model: decode_coverage_model(reader)?,
        modulus: reader.u32()?,
        fence: CoverageFence::new(decode_usizes(reader, limits.coverage.max_vertices)?)?,
        failable_vertices: decode_indices(reader, vertex_count, limits.coverage.max_vertices)?,
        failure_budget: reader.usize()?,
        source: decode_source(reader, limits)?,
    })
}

fn decode_coverage_model(reader: &mut Reader<'_>) -> Result<PlanarCoverageModel> {
    let broadcast = f64::from_bits(reader.u64()?);
    let sensing = f64::from_bits(reader.u64()?);
    PlanarCoverageModel::new(broadcast, sensing)
}

fn decode_coverage_states(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: CoverageSynthesisLimits,
) -> Result<Vec<CoverageState>> {
    let count = reader.bounded_usize("state count", limits.coverage.max_states)?;
    let mut states = Vec::with_capacity(count);
    for _ in 0..count {
        states.push(decode_coverage_state(reader, vertex_count, limits)?);
    }
    Ok(states)
}

fn decode_coverage_state(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageState> {
    Ok(CoverageState {
        scenario: reader.u64()?,
        step: reader.u64()?,
        base_vertices: decode_indices(reader, vertex_count, limits.coverage.max_vertices)?,
        possible_edges: decode_edges(reader, vertex_count, limits.coverage.max_edges)?,
    })
}

pub(super) fn decode_coverage_actions(
    reader: &mut Reader<'_>,
    state_count: usize,
    limits: CoverageSynthesisLimits,
) -> Result<Vec<CoverageAction>> {
    let count = reader.bounded_usize("action count", limits.coverage.max_actions)?;
    let mut actions = Vec::with_capacity(count);
    for _ in 0..count {
        actions.push(CoverageAction {
            vertex: reader.usize()?,
            cost: reader.u64()?,
            states: decode_indices(reader, state_count, state_count)?,
        });
    }
    Ok(actions)
}

pub(super) fn decode_coverage_search(
    reader: &mut Reader<'_>,
    action_count: usize,
) -> Result<DecodedCoverageSearch> {
    let max_activations = reader.usize()?;
    let limits = decode_work_limits(reader)?;
    let selection = decode_selection(reader, action_count)?;
    let work = decode_producer_work(reader)?;
    Ok(DecodedCoverageSearch {
        max_activations,
        oracle_limit: limits.oracle,
        node_limit: limits.nodes,
        status: selection.status,
        selected: selection.selected,
        lower_bound_cost: selection.lower_bound_cost,
        upper_bound_cost: selection.upper_bound_cost,
        producer_oracle_calls: work.oracle_calls,
        producer_search_nodes: work.search_nodes,
        producer_cache_hits: work.cache_hits,
    })
}

fn decode_work_limits(reader: &mut Reader<'_>) -> Result<CoverageWorkLimits> {
    Ok(CoverageWorkLimits {
        oracle: reader.usize()?,
        nodes: reader.usize()?,
    })
}

fn decode_selection(reader: &mut Reader<'_>, action_count: usize) -> Result<CoverageSelectionData> {
    Ok(CoverageSelectionData {
        status: CoverageSynthesisStatus::from_code(reader.u8()?)?,
        selected: decode_indices(reader, action_count, action_count)?,
        lower_bound_cost: reader.optional_u64()?,
        upper_bound_cost: reader.optional_u64()?,
    })
}

fn decode_producer_work(reader: &mut Reader<'_>) -> Result<CoverageProducerWork> {
    Ok(CoverageProducerWork {
        oracle_calls: reader.usize()?,
        search_nodes: reader.usize()?,
        cache_hits: reader.usize()?,
    })
}

pub(super) fn decode_coverage_trailer(reader: &mut Reader<'_>) -> Result<[u8; 32]> {
    let digest = reader.array32()?;
    if reader.remaining() != 0 {
        Err(Error::InvalidInput(
            "trailing bytes follow the coverage artifact".into(),
        ))
    } else {
        Ok(digest)
    }
}

fn decode_source(
    reader: &mut Reader<'_>,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageSource> {
    match reader.u8()? {
        0 => Ok(CoverageSource::Finite),
        1 => decode_affine_source(reader, limits),
        _ => Err(Error::InvalidInput(
            "coverage source kind is invalid".into(),
        )),
    }
}

fn decode_affine_source(
    reader: &mut Reader<'_>,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageSource> {
    let scenario = reader.u64()?;
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let edges = decode_affine_edges(reader, limits)?;
    Ok(CoverageSource::Affine {
        scenario,
        edges,
        start,
        end,
    })
}

fn decode_affine_edges(
    reader: &mut Reader<'_>,
    limits: CoverageSynthesisLimits,
) -> Result<Vec<KineticEdge>> {
    let count = reader.bounded_usize("affine edge count", limits.kinetic.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(Error::InvalidInput(
            "coverage affine edges exceed the remaining bytes".into(),
        ));
    }
    (0..count)
        .map(|_| {
            Ok(KineticEdge {
                u: reader.usize()?,
                v: reader.usize()?,
                intercept: f64::from_bits(reader.u64()?),
                velocity: f64::from_bits(reader.u64()?),
            })
        })
        .collect()
}
