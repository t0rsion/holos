use std::collections::BTreeSet;

use crate::{ProofBar, ProofError};

use super::model::Graph;

pub(crate) fn h0_diagram(
    graph: &Graph,
    threshold: Option<f64>,
) -> Result<Vec<ProofBar>, ProofError> {
    let threshold = checked_threshold(threshold)?;
    let mut edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .copied()
        .collect();
    edges.sort_by(|left, right| {
        left.value
            .total_cmp(&right.value)
            .then_with(|| edge_rank([right.u, right.v]).cmp(&edge_rank([left.u, left.v])))
    });
    let mut parent: Vec<_> = (0..graph.vertex_count).collect();
    let mut diagram = Vec::new();
    for edge in edges {
        let left = find(&mut parent, edge.u);
        let right = find(&mut parent, edge.v);
        if left == right {
            continue;
        }
        parent[right] = left;
        if edge.value > 0.0 {
            diagram.push(ProofBar {
                dimension: 0,
                birth: 0.0,
                death: edge.value,
            });
        }
    }
    let roots: BTreeSet<_> = (0..graph.vertex_count)
        .map(|vertex| find(&mut parent, vertex))
        .collect();
    diagram.extend(roots.into_iter().map(|_| ProofBar {
        dimension: 0,
        birth: 0.0,
        death: f64::INFINITY,
    }));
    Ok(diagram)
}

fn find(parent: &mut [usize], mut vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[vertex] != vertex {
        let next = parent[vertex];
        parent[vertex] = root;
        vertex = next;
    }
    root
}

pub(crate) fn check_diagram(diagram: &[ProofBar]) -> Result<(), ProofError> {
    for bar in diagram {
        if bar.dimension > 1
            || !bar.birth.is_finite()
            || bar.birth < 0.0
            || bar.death.is_nan()
            || bar.death < 0.0
            || bar.death <= bar.birth
        {
            return Err(ProofError::new("diagram contains an invalid bar"));
        }
    }
    let mut canonical = diagram.to_vec();
    canonicalize_diagram(&mut canonical);
    if !diagrams_equal(&canonical, diagram) {
        return Err(ProofError::new("diagram bars are not canonical"));
    }
    Ok(())
}

pub(crate) fn canonicalize_diagram(diagram: &mut [ProofBar]) {
    diagram.sort_by(|left, right| {
        left.dimension
            .cmp(&right.dimension)
            .then(left.birth.total_cmp(&right.birth))
            .then(left.death.total_cmp(&right.death))
    });
}

pub(crate) fn diagrams_equal(left: &[ProofBar], right: &[ProofBar]) -> bool {
    left.len() == right.len()
        && left.iter().zip(right).all(|(left, right)| {
            left.dimension == right.dimension
                && left.birth.to_bits() == right.birth.to_bits()
                && left.death.to_bits() == right.death.to_bits()
        })
}

pub(crate) fn checked_threshold(threshold: Option<f64>) -> Result<f64, ProofError> {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    if threshold.is_nan() || threshold < 0.0 {
        return Err(ProofError::new(format!(
            "threshold must be non-negative, got {threshold}"
        )));
    }
    Ok(threshold)
}

fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * v.saturating_sub(1) as u128 / 2 + u as u128
}
