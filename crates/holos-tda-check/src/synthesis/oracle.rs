use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use num_rational::BigRational;

use crate::cohomology::{Edge, MapTerm, Space};
use crate::{ProofError, ProofLimits};

use super::model::{AffineEdge, Claim, FORMAT_MAX_STATES, Source, State};

pub(super) fn validate_source(claim: &Claim, limits: ProofLimits) -> Result<(), ProofError> {
    let Source::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    } = &claim.source
    else {
        return Ok(());
    };
    validate_affine(claim.vertex_count, edges, *start, *end)?;
    let graphs = complete_threshold_graphs(edges, *start, *end, claim.scale, limits)?;
    let mut expected = Vec::new();
    for (step, graph) in graphs.into_iter().enumerate() {
        let space = Space::build(
            claim.vertex_count,
            claim.dimension,
            &graph,
            claim.modulus,
            limits,
        )?;
        if space.rank() <= *maximum_rank {
            continue;
        }
        expected.push(State {
            scenario: *scenario,
            step: step as u64,
            target_space: space.id(
                claim.vertex_count,
                claim.dimension,
                claim.scale,
                claim.modulus,
                &graph,
            ),
            target: (0..space.rank())
                .map(|target| {
                    vec![MapTerm {
                        target,
                        coefficient: 1,
                    }]
                })
                .collect(),
            edges: graph,
            max_surviving_rank: *maximum_rank,
        });
    }
    if expected != claim.states {
        return Err(ProofError::new(
            "synthesis states are not the complete affine threshold schedule",
        ));
    }
    Ok(())
}

pub(super) fn validate_affine(
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

pub(super) fn validate_affine_interval(start: f64, end: f64) -> Result<(), ProofError> {
    if !start.is_finite() || !end.is_finite() || start >= end {
        Err(ProofError::new("synthesis affine interval is invalid"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_trajectory(
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
            "synthesis affine edge trajectory is not canonical",
        ));
    }
    validate_trajectory_weight(trajectory, start)?;
    validate_trajectory_weight(trajectory, end)
}

pub(super) fn validate_trajectory_weight(
    trajectory: &AffineEdge,
    time: f64,
) -> Result<(), ProofError> {
    let weight = trajectory.intercept + trajectory.velocity * time;
    if !weight.is_finite() || weight < 0.0 {
        Err(ProofError::new(
            "synthesis affine edge weight leaves its valid range",
        ))
    } else {
        Ok(())
    }
}

fn complete_threshold_graphs(
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
        .ok_or_else(|| ProofError::new("synthesis affine state count overflows"))?;
    if graph_count > limits.max_snapshots.min(FORMAT_MAX_STATES) {
        return Err(ProofError::new(
            "synthesis affine schedule exceeds its state limit",
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

fn active_edges(edges: &[AffineEdge], time: &BigRational, scale: &BigRational) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| rational(edge.intercept) + rational(edge.velocity) * time <= *scale)
        .map(|edge| edge.edge)
        .collect()
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

fn midpoint(left: &BigRational, right: &BigRational) -> BigRational {
    (left + right) / BigRational::from_integer(2.into())
}

pub(super) struct Oracle<'a> {
    claim: &'a Claim,
    spaces: Vec<Space>,
    targets: Vec<Vec<Vec<MapTerm>>>,
    limits: ProofLimits,
    cache: RefCell<BTreeMap<(usize, Vec<usize>), usize>>,
}

impl<'a> Oracle<'a> {
    pub(super) fn build(claim: &'a Claim, limits: ProofLimits) -> Result<Self, ProofError> {
        let mut spaces = Vec::with_capacity(claim.states.len());
        let mut targets = Vec::with_capacity(claim.states.len());
        for state in &claim.states {
            let space = Space::build(
                claim.vertex_count,
                claim.dimension,
                &state.edges,
                claim.modulus,
                limits,
            )?;
            if space.id(
                claim.vertex_count,
                claim.dimension,
                claim.scale,
                claim.modulus,
                &state.edges,
            ) != state.target_space
            {
                return Err(ProofError::new(
                    "synthesis target is bound to a different active complex",
                ));
            }
            let target = space.canonical_subspace(&state.target, claim.modulus)?;
            if target != state.target
                || target.is_empty()
                || state.max_surviving_rank >= target.len()
            {
                return Err(ProofError::new(
                    "synthesis target is not a canonical constrained subspace",
                ));
            }
            spaces.push(space);
            targets.push(target);
        }
        Ok(Self {
            claim,
            spaces,
            targets,
            limits,
            cache: RefCell::new(BTreeMap::new()),
        })
    }

    pub(super) fn target_ranks(&self) -> Vec<usize> {
        self.targets.iter().map(Vec::len).collect()
    }

    pub(super) fn survives(&self, selected: &[usize]) -> Result<bool, ProofError> {
        Ok(self
            .intersection_ranks(selected)?
            .iter()
            .zip(&self.claim.states)
            .any(|(rank, state)| *rank > state.max_surviving_rank))
    }

    pub(super) fn intersection_ranks(&self, selected: &[usize]) -> Result<Vec<usize>, ProofError> {
        (0..self.claim.states.len())
            .map(|state| self.intersection_rank(state, selected))
            .collect()
    }

    fn intersection_rank(&self, state: usize, selected: &[usize]) -> Result<usize, ProofError> {
        let relevant = selected
            .iter()
            .copied()
            .filter(|action| {
                self.claim.actions[*action]
                    .states
                    .binary_search(&state)
                    .is_ok()
            })
            .collect::<Vec<_>>();
        let key = (state, relevant.clone());
        if let Some(rank) = self.cache.borrow().get(&key) {
            return Ok(*rank);
        }
        let mut edges = self.claim.states[state].edges.clone();
        for action in relevant {
            edges.push(self.claim.actions[action].edge);
        }
        edges.sort();
        edges.dedup();
        let source = Space::build(
            self.claim.vertex_count,
            self.claim.dimension,
            &edges,
            self.claim.modulus,
            self.limits,
        )?;
        let rank = self.spaces[state].subspace_intersection_rank_from(
            &source,
            &self.targets[state],
            self.claim.modulus,
        )?;
        self.cache.borrow_mut().insert(key, rank);
        Ok(rank)
    }
}
