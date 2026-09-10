use crate::{
    AtlasArtifact, AtlasDecodeLimits, CertificateLimits, SparseDistanceMatrix, TopologyEvent,
    TopologyEventKind, UpdateMode,
};

use super::model::{TrajectoryDecodeLimits, TrajectoryError, TrajectoryStep};
use super::primitives::Reader;
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};

pub(super) struct TrajectoryHeader {
    pub(super) step_count: usize,
    pub(super) initial_atlas_bytes: usize,
}

pub(super) struct TrajectoryDecodeContext<'a> {
    pub(super) limits: TrajectoryDecodeLimits,
    pub(super) atlas_limits: AtlasDecodeLimits,
    pub(super) certificate_limits: CertificateLimits,
    pub(super) total_edges: &'a mut usize,
    pub(super) total_events: &'a mut usize,
}

pub(super) fn check_envelope_size(bytes: &[u8], max_bytes: usize) -> Result<(), TrajectoryError> {
    if bytes.len() > max_bytes {
        return Err(TrajectoryError::new(format!(
            "{} bytes exceed the decoder limit {max_bytes}",
            bytes.len()
        )));
    }
    Ok(())
}

pub(super) fn decode_trajectory_header(
    reader: &mut Reader<'_>,
    limits: TrajectoryDecodeLimits,
) -> Result<TrajectoryHeader, TrajectoryError> {
    check_trajectory_identity(reader)?;
    Ok(TrajectoryHeader {
        step_count: reader.bounded_usize("step count", limits.max_steps)?,
        initial_atlas_bytes: reader
            .bounded_usize("initial atlas byte count", limits.max_atlas_bytes)?,
    })
}

fn check_trajectory_identity(reader: &mut Reader<'_>) -> Result<(), TrajectoryError> {
    if reader.take(8)? != MAGIC {
        return Err(TrajectoryError::new("wrong magic bytes"));
    }
    let version = reader.u16()?;
    if version != WIRE_VERSION {
        return Err(TrajectoryError::new(format!(
            "unsupported wire version {version}"
        )));
    }
    if reader.u8()? != F64_BITS_CODEC {
        return Err(TrajectoryError::new("unsupported scalar codec"));
    }
    Ok(())
}

pub(super) fn decode_nested_atlas(
    reader: &mut Reader<'_>,
    byte_count: usize,
    limits: TrajectoryDecodeLimits,
    atlas_limits: AtlasDecodeLimits,
    certificate_limits: CertificateLimits,
) -> Result<AtlasArtifact, TrajectoryError> {
    decode_atlas(
        reader.take(byte_count)?,
        limits,
        atlas_limits,
        certificate_limits,
    )
}

pub(super) fn decode_trajectory_steps(
    reader: &mut Reader<'_>,
    count: usize,
    context: &mut TrajectoryDecodeContext<'_>,
) -> Result<Vec<TrajectoryStep>, TrajectoryError> {
    let mut steps = Vec::with_capacity(count);
    for _ in 0..count {
        steps.push(decode_trajectory_step(reader, context)?);
    }
    Ok(steps)
}

fn decode_trajectory_step(
    reader: &mut Reader<'_>,
    context: &mut TrajectoryDecodeContext<'_>,
) -> Result<TrajectoryStep, TrajectoryError> {
    let input = decode_graph(reader, context.limits, context.total_edges)?;
    let mode = decode_mode(reader.u8()?)?;
    let event_count = reader.usize()?;
    add_event_count(
        context.total_events,
        event_count,
        context.limits.max_total_events,
    )?;
    let events = decode_events(reader, event_count)?;
    let checkpoint_bytes =
        reader.bounded_usize("checkpoint byte count", context.limits.max_atlas_bytes)?;
    let checkpoint = decode_checkpoint(reader, checkpoint_bytes, context)?;
    Ok(TrajectoryStep {
        input,
        mode,
        events,
        checkpoint,
    })
}

fn add_event_count(total: &mut usize, count: usize, limit: usize) -> Result<(), TrajectoryError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| TrajectoryError::new("event count overflows usize"))?;
    if *total > limit {
        return Err(TrajectoryError::new(format!(
            "{} events exceed the decoder limit {limit}",
            *total
        )));
    }
    Ok(())
}

fn decode_events(
    reader: &mut Reader<'_>,
    count: usize,
) -> Result<Vec<TopologyEvent>, TrajectoryError> {
    check_event_bytes(reader, count)?;
    let mut events = Vec::with_capacity(count);
    for _ in 0..count {
        events.push(decode_event(reader)?);
    }
    Ok(events)
}

fn check_event_bytes(reader: &Reader<'_>, count: usize) -> Result<(), TrajectoryError> {
    let minimum = count
        .checked_mul(7)
        .ok_or_else(|| TrajectoryError::new("event bytes overflow usize"))?;
    if minimum > reader.remaining() {
        return Err(TrajectoryError::new("events exceed the remaining bytes"));
    }
    Ok(())
}

fn decode_checkpoint(
    reader: &mut Reader<'_>,
    byte_count: usize,
    context: &TrajectoryDecodeContext<'_>,
) -> Result<Option<AtlasArtifact>, TrajectoryError> {
    if byte_count == 0 {
        return Ok(None);
    }
    decode_nested_atlas(
        reader,
        byte_count,
        context.limits,
        context.atlas_limits,
        context.certificate_limits,
    )
    .map(Some)
}

pub(super) fn check_no_trailing_bytes(reader: &Reader<'_>) -> Result<(), TrajectoryError> {
    if reader.remaining() != 0 {
        return Err(TrajectoryError::new(format!(
            "{} trailing bytes after the envelope",
            reader.remaining()
        )));
    }
    Ok(())
}

fn decode_atlas(
    bytes: &[u8],
    trace_limits: TrajectoryDecodeLimits,
    mut atlas_limits: AtlasDecodeLimits,
    mut certificate_limits: CertificateLimits,
) -> std::result::Result<AtlasArtifact, TrajectoryError> {
    atlas_limits.max_bytes = atlas_limits
        .max_bytes
        .min(trace_limits.max_atlas_bytes)
        .min(bytes.len());
    certificate_limits.max_bytes = certificate_limits.max_bytes.min(bytes.len());
    AtlasArtifact::decode(bytes, atlas_limits, certificate_limits)
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

pub(super) fn decode_graph(
    reader: &mut Reader<'_>,
    limits: TrajectoryDecodeLimits,
    total_edges: &mut usize,
) -> std::result::Result<SparseDistanceMatrix, TrajectoryError> {
    let (vertices, edges) = decode_graph_counts(reader, limits)?;
    add_graph_edges(total_edges, edges, limits.max_total_edges)?;
    check_graph_bytes(reader, edges)?;
    let triplets = decode_graph_triplets(reader, vertices, edges)?;
    SparseDistanceMatrix::from_triplets(vertices, &triplets)
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

fn decode_graph_counts(
    reader: &mut Reader<'_>,
    limits: TrajectoryDecodeLimits,
) -> Result<(usize, usize), TrajectoryError> {
    let vertices = reader.bounded_usize("graph vertex count", limits.max_vertices)?;
    let possible = vertices
        .checked_mul(vertices.saturating_sub(1))
        .map(|value| value / 2)
        .unwrap_or(usize::MAX);
    let edges = reader.bounded_usize("graph edge count", possible)?;
    Ok((vertices, edges))
}

fn add_graph_edges(total: &mut usize, count: usize, limit: usize) -> Result<(), TrajectoryError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| TrajectoryError::new("total edge count overflows usize"))?;
    if *total > limit {
        return Err(TrajectoryError::new(format!(
            "{} edges exceed the decoder limit {limit}",
            *total
        )));
    }
    Ok(())
}

fn check_graph_bytes(reader: &Reader<'_>, edges: usize) -> Result<(), TrajectoryError> {
    let bytes = edges
        .checked_mul(24)
        .ok_or_else(|| TrajectoryError::new("graph edge bytes overflow usize"))?;
    if bytes > reader.remaining() {
        return Err(TrajectoryError::new(
            "graph edges exceed the remaining bytes",
        ));
    }
    Ok(())
}

fn decode_graph_triplets(
    reader: &mut Reader<'_>,
    vertices: usize,
    edges: usize,
) -> Result<Vec<(usize, usize, f64)>, TrajectoryError> {
    let mut triplets = Vec::with_capacity(edges);
    let mut previous = None;
    for _ in 0..edges {
        let triplet = decode_graph_triplet(reader)?;
        check_graph_triplet(triplet, vertices, previous)?;
        previous = Some((triplet.0, triplet.1));
        triplets.push(triplet);
    }
    Ok(triplets)
}

fn decode_graph_triplet(reader: &mut Reader<'_>) -> Result<(usize, usize, f64), TrajectoryError> {
    Ok((
        reader.usize()?,
        reader.usize()?,
        f64::from_bits(reader.u64()?),
    ))
}

fn check_graph_triplet(
    triplet: (usize, usize, f64),
    vertices: usize,
    previous: Option<(usize, usize)>,
) -> Result<(), TrajectoryError> {
    let (u, v, value) = triplet;
    if u >= v || v >= vertices || previous.is_some_and(|edge| edge >= (u, v)) {
        return Err(TrajectoryError::new(
            "graph edges are not in strict canonical order",
        ));
    }
    if !value.is_finite() || value < 0.0 || (value == 0.0 && value.to_bits() != 0) {
        return Err(TrajectoryError::new(
            "graph edge weight is not canonical and non-negative",
        ));
    }
    Ok(())
}

fn decode_event(reader: &mut Reader<'_>) -> std::result::Result<TopologyEvent, TrajectoryError> {
    Ok(TopologyEvent {
        kind: decode_event_kind(reader.u8()?)?,
        first: reader.optional_edge()?,
        second: reader.optional_edge()?,
        old_first: reader.optional_f64()?,
        new_first: reader.optional_f64()?,
        old_second: reader.optional_f64()?,
        new_second: reader.optional_f64()?,
    })
}

fn decode_mode(tag: u8) -> std::result::Result<UpdateMode, TrajectoryError> {
    match tag {
        0 => Ok(UpdateMode::Reused),
        1 => Ok(UpdateMode::Recomputed),
        _ => Err(TrajectoryError::new(format!(
            "unknown update-mode tag {tag}"
        ))),
    }
}

fn decode_event_kind(tag: u8) -> std::result::Result<TopologyEventKind, TrajectoryError> {
    match tag {
        0 => Ok(TopologyEventKind::VertexSetChanged),
        1 => Ok(TopologyEventKind::EdgeSetChanged),
        2 => Ok(TopologyEventKind::ThresholdCrossing),
        3 => Ok(TopologyEventKind::EqualitySplit),
        4 => Ok(TopologyEventKind::EqualityMerge),
        5 => Ok(TopologyEventKind::OrderSwap),
        _ => Err(TrajectoryError::new(format!(
            "unknown topology-event tag {tag}"
        ))),
    }
}
