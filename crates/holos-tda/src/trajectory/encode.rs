use crate::{
    AtlasArtifact, EdgeKey, SparseDistanceMatrix, TopologyEvent, TopologyEventKind, UpdateMode,
};

use super::model::{TrajectoryError, TrajectoryStep};
use super::primitives::{put_optional_f64, put_u16, put_u64, put_usize};
use super::{F64_BITS_CODEC, MAGIC, WIRE_VERSION};

pub(super) fn encode_atlas(atlas: &AtlasArtifact) -> Result<Vec<u8>, TrajectoryError> {
    atlas
        .encode()
        .map_err(|error| TrajectoryError::new(error.to_string()))
}

pub(super) fn encode_checkpoints(
    steps: &[TrajectoryStep],
) -> Result<Vec<Option<Vec<u8>>>, TrajectoryError> {
    let mut checkpoints = Vec::with_capacity(steps.len());
    for step in steps {
        checkpoints.push(step.checkpoint.as_ref().map(encode_atlas).transpose()?);
    }
    Ok(checkpoints)
}

pub(super) fn encode_trajectory_header(
    out: &mut Vec<u8>,
    step_count: usize,
    initial_atlas_bytes: usize,
) -> Result<(), TrajectoryError> {
    out.extend_from_slice(MAGIC);
    put_u16(out, WIRE_VERSION);
    out.push(F64_BITS_CODEC);
    put_usize(out, step_count, "step count")?;
    put_usize(out, initial_atlas_bytes, "initial atlas byte count")?;
    Ok(())
}

pub(super) fn encode_trajectory_step(
    out: &mut Vec<u8>,
    step: &TrajectoryStep,
    checkpoint: Option<&[u8]>,
) -> Result<(), TrajectoryError> {
    encode_graph(out, &step.input)?;
    out.push(mode_tag(step.mode));
    put_usize(out, step.events.len(), "event count")?;
    encode_events(out, &step.events)?;
    put_usize(
        out,
        checkpoint.map_or(0, <[u8]>::len),
        "checkpoint byte count",
    )?;
    if let Some(checkpoint) = checkpoint {
        out.extend_from_slice(checkpoint);
    }
    Ok(())
}

fn encode_events(out: &mut Vec<u8>, events: &[TopologyEvent]) -> Result<(), TrajectoryError> {
    for event in events {
        encode_event(out, event)?;
    }
    Ok(())
}

pub(super) fn encode_graph(
    out: &mut Vec<u8>,
    input: &SparseDistanceMatrix,
) -> std::result::Result<(), TrajectoryError> {
    let edges: Vec<_> = input.edges().collect();
    put_usize(out, input.len(), "graph vertex count")?;
    put_usize(out, edges.len(), "graph edge count")?;
    for (u, v, value) in edges {
        put_usize(out, u, "edge endpoint")?;
        put_usize(out, v, "edge endpoint")?;
        put_u64(out, value.to_bits());
    }
    Ok(())
}

fn encode_event(
    out: &mut Vec<u8>,
    event: &TopologyEvent,
) -> std::result::Result<(), TrajectoryError> {
    out.push(event_kind_tag(event.kind));
    put_optional_edge(out, event.first)?;
    put_optional_edge(out, event.second)?;
    put_optional_f64(out, event.old_first);
    put_optional_f64(out, event.new_first);
    put_optional_f64(out, event.old_second);
    put_optional_f64(out, event.new_second);
    Ok(())
}

fn mode_tag(mode: UpdateMode) -> u8 {
    match mode {
        UpdateMode::Reused => 0,
        UpdateMode::Recomputed => 1,
    }
}

fn event_kind_tag(kind: TopologyEventKind) -> u8 {
    match kind {
        TopologyEventKind::VertexSetChanged => 0,
        TopologyEventKind::EdgeSetChanged => 1,
        TopologyEventKind::ThresholdCrossing => 2,
        TopologyEventKind::EqualitySplit => 3,
        TopologyEventKind::EqualityMerge => 4,
        TopologyEventKind::OrderSwap => 5,
    }
}

fn put_optional_edge(
    out: &mut Vec<u8>,
    edge: Option<EdgeKey>,
) -> std::result::Result<(), TrajectoryError> {
    match edge {
        None => out.push(0),
        Some(edge) => {
            out.push(1);
            put_usize(out, edge.u, "event edge endpoint")?;
            put_usize(out, edge.v, "event edge endpoint")?;
        }
    }
    Ok(())
}
