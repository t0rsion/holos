use std::collections::BTreeSet;

use num_rational::BigRational;

use crate::{ProofError, ProofLimits};

use super::FORMAT_MAX_STATES;
use super::model::{AffineEdge, Claim, Edge, Source, State};
use super::wire::Reader;

pub(crate) fn decode_source(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Source, ProofError> {
    match reader.u8()? {
        0 => Ok(Source::Finite),
        1 => decode_affine_source(reader, limits),
        _ => Err(ProofError::new("coverage source kind is invalid")),
    }
}

pub(crate) fn decode_affine_source(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Source, ProofError> {
    let scenario = reader.u64()?;
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let count = reader.bounded_usize("affine edge count", limits.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(ProofError::new(
            "coverage affine edges exceed the remaining bytes",
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
    })
}

pub(crate) fn decode_affine_edge(reader: &mut Reader<'_>) -> Result<AffineEdge, ProofError> {
    Ok(AffineEdge {
        edge: Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
}

pub(crate) fn validate_source(claim: &Claim, limits: ProofLimits) -> Result<(), ProofError> {
    let Source::Affine {
        scenario,
        edges,
        start,
        end,
    } = &claim.source
    else {
        return Ok(());
    };
    validate_affine(claim.vertex_count, edges, *start, *end)?;
    let base = claim
        .states
        .first()
        .map(|state| state.base.clone())
        .ok_or_else(|| ProofError::new("coverage affine source has no states"))?;
    if claim.states.iter().any(|state| state.base != base) {
        return Err(ProofError::new(
            "coverage affine source changes its base sensor set",
        ));
    }
    let graphs = complete_threshold_graphs(edges, *start, *end, claim.broadcast_radius, limits)?;
    let expected = graphs
        .into_iter()
        .enumerate()
        .map(|(step, edges)| State {
            scenario: *scenario,
            step: step as u64,
            base: base.clone(),
            edges,
        })
        .collect::<Vec<_>>();
    if expected != claim.states {
        return Err(ProofError::new(
            "coverage states are not the complete affine threshold schedule",
        ));
    }
    Ok(())
}

pub(crate) fn validate_affine(
    vertex_count: usize,
    edges: &[AffineEdge],
    start: f64,
    end: f64,
) -> Result<(), ProofError> {
    validate_affine_interval(start, end)?;
    let mut previous = None;
    for trajectory in edges {
        validate_trajectory(trajectory, previous, vertex_count, start, end)?;
        previous = Some(trajectory.edge);
    }
    Ok(())
}

pub(crate) fn validate_affine_interval(start: f64, end: f64) -> Result<(), ProofError> {
    if !start.is_finite() || !end.is_finite() || start >= end {
        Err(ProofError::new("coverage affine interval is invalid"))
    } else {
        Ok(())
    }
}

pub(crate) fn validate_trajectory(
    trajectory: &AffineEdge,
    previous: Option<Edge>,
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
            "coverage affine edge trajectory is not canonical",
        ));
    }
    validate_trajectory_weight(trajectory, start)?;
    validate_trajectory_weight(trajectory, end)
}

pub(crate) fn validate_trajectory_weight(
    trajectory: &AffineEdge,
    time: f64,
) -> Result<(), ProofError> {
    let weight = trajectory.intercept + trajectory.velocity * time;
    if !weight.is_finite() || weight < 0.0 {
        Err(ProofError::new(
            "coverage affine edge weight leaves its valid range",
        ))
    } else {
        Ok(())
    }
}

pub(crate) fn complete_threshold_graphs(
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    limits: ProofLimits,
) -> Result<Vec<Vec<Edge>>, ProofError> {
    let start = rational(start);
    let end = rational(end);
    let scale = rational(scale);
    let mut events = BTreeSet::new();
    for edge in edges {
        let velocity = rational(edge.velocity);
        if velocity == BigRational::from_integer(0.into()) {
            continue;
        }
        let time = (&scale - rational(edge.intercept)) / velocity;
        if start < time && time < end {
            events.insert(time);
        }
    }
    let events = events.into_iter().collect::<Vec<_>>();
    let graph_count = events
        .len()
        .checked_mul(2)
        .and_then(|count| count.checked_add(3))
        .ok_or_else(|| ProofError::new("coverage affine state count overflows"))?;
    if graph_count > limits.max_snapshots.min(FORMAT_MAX_STATES) {
        return Err(ProofError::new(
            "coverage affine schedule exceeds its state limit",
        ));
    }
    let mut graphs = Vec::with_capacity(graph_count);
    graphs.push(active_edges(edges, &start, &scale));
    for position in 0..=events.len() {
        let left = if position == 0 {
            &start
        } else {
            &events[position - 1]
        };
        let right = events.get(position).unwrap_or(&end);
        graphs.push(active_edges(edges, &midpoint(left, right), &scale));
        if let Some(time) = events.get(position) {
            graphs.push(active_edges(edges, time, &scale));
        }
    }
    graphs.push(active_edges(edges, &end, &scale));
    Ok(graphs)
}

pub(crate) fn active_edges(
    edges: &[AffineEdge],
    time: &BigRational,
    scale: &BigRational,
) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| rational(edge.intercept) + rational(edge.velocity) * time <= *scale)
        .map(|edge| edge.edge)
        .collect()
}

pub(crate) fn validate_radii(broadcast: f64, sensing: f64) -> Result<(), ProofError> {
    if !broadcast.is_finite() || !sensing.is_finite() || broadcast <= 0.0 || sensing <= 0.0 {
        return Err(ProofError::new(
            "coverage radii must be finite and positive",
        ));
    }
    let broadcast = rational(broadcast);
    let sensing = rational(sensing);
    if BigRational::from_integer(3.into()) * &sensing * sensing < broadcast.clone() * broadcast {
        return Err(ProofError::new(
            "coverage radii violate the exact controlled-boundary inequality",
        ));
    }
    Ok(())
}

pub(crate) fn canonical_fence(vertices: &[usize]) -> Vec<usize> {
    let forward = rotate_to_minimum(vertices);
    let mut reversed = vertices.to_vec();
    reversed.reverse();
    let reversed = rotate_to_minimum(&reversed);
    forward.min(reversed)
}

pub(crate) fn rotate_to_minimum(values: &[usize]) -> Vec<usize> {
    let position = values
        .iter()
        .enumerate()
        .min_by_key(|(_, value)| **value)
        .map(|(position, _)| position)
        .unwrap_or(0);
    values[position..]
        .iter()
        .chain(&values[..position])
        .copied()
        .collect()
}

pub(crate) fn cycle_pairs(vertices: &[usize]) -> impl Iterator<Item = (usize, usize)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
}

pub(crate) fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

pub(crate) fn midpoint(left: &BigRational, right: &BigRational) -> BigRational {
    (left + right) / BigRational::from_integer(2.into())
}
