use std::collections::BTreeSet;

use num_rational::BigRational;

use crate::{ProofError, ProofLimits};

use super::model::{AffineEdge, ExactSchedule};

pub(crate) fn exact_schedule(
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    limits: ProofLimits,
) -> Result<ExactSchedule, ProofError> {
    validate_pair_count(edges.len(), limits)?;
    let start = rational(start);
    let end = rational(end);
    let scale = rational(scale);
    let coefficients = edges
        .iter()
        .map(|edge| (rational(edge.intercept), rational(edge.velocity)))
        .collect::<Vec<_>>();
    let (mut events, persistent_ties) = pair_events(&coefficients, &start, &end);
    threshold_events(&mut events, &coefficients, &start, &end, &scale);
    if events.len() > limits.max_snapshots {
        return Err(ProofError::new(
            "kinetic zigzag event count exceeds its limit",
        ));
    }
    Ok(ExactSchedule {
        events: events.into_iter().collect(),
        persistent_ties,
    })
}

fn validate_pair_count(count: usize, limits: ProofLimits) -> Result<(), ProofError> {
    let pairs = count
        .checked_mul(count.saturating_sub(1))
        .map(|value| value / 2)
        .ok_or_else(|| ProofError::new("kinetic zigzag pair count overflows"))?;
    if pairs > limits.max_references {
        Err(ProofError::new(
            "kinetic zigzag pair count exceeds its limit",
        ))
    } else {
        Ok(())
    }
}

fn pair_events(
    coefficients: &[(BigRational, BigRational)],
    start: &BigRational,
    end: &BigRational,
) -> (BTreeSet<BigRational>, usize) {
    let mut events = BTreeSet::new();
    let mut persistent_ties = 0usize;
    for left in 0..coefficients.len() {
        for right in left + 1..coefficients.len() {
            let numerator = &coefficients[right].0 - &coefficients[left].0;
            let denominator = &coefficients[left].1 - &coefficients[right].1;
            if denominator == BigRational::from_integer(0.into()) {
                if numerator == BigRational::from_integer(0.into()) {
                    persistent_ties += 1;
                }
            } else {
                let time = numerator / denominator;
                if start < &time && &time < end {
                    events.insert(time);
                }
            }
        }
    }
    (events, persistent_ties)
}

fn threshold_events(
    events: &mut BTreeSet<BigRational>,
    coefficients: &[(BigRational, BigRational)],
    start: &BigRational,
    end: &BigRational,
    scale: &BigRational,
) {
    for (intercept, velocity) in coefficients {
        if velocity != &BigRational::from_integer(0.into()) {
            let time = (scale - intercept) / velocity;
            if start < &time && &time < end {
                events.insert(time);
            }
        }
    }
}

pub(crate) fn event_graphs(
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    events: &[BigRational],
) -> Vec<Vec<crate::cohomology::Edge>> {
    let start = rational(start);
    let end = rational(end);
    let scale = rational(scale);
    let mut graphs = Vec::with_capacity(events.len() * 2 + 1);
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
    graphs
}

fn active_edges(
    edges: &[AffineEdge],
    time: &BigRational,
    scale: &BigRational,
) -> Vec<crate::cohomology::Edge> {
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
