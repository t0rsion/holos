use crate::ProofError;
use crate::cohomology::{Edge, Space};

use super::BipersistenceProofLimits;
use super::claims::{Claim, NodeClaim, WeightedEdge};
use super::grid::validate_axes;

pub(super) struct CheckedNode {
    pub(super) space: Space,
    pub(super) space_id: [u8; 32],
    pub(super) edges: Vec<Edge>,
}

pub(super) fn validate_grid(
    claim: &Claim,
    limits: BipersistenceProofLimits,
) -> Result<(Vec<f64>, Vec<usize>), ProofError> {
    let scales = validate_axes(
        claim.vertex_count,
        claim.threshold_bits,
        &claim.scale_bits,
        &claim.minimum_degrees,
    )?;
    let minimum_degrees = claim.minimum_degrees.clone();
    let node_count = scales
        .len()
        .checked_mul(minimum_degrees.len())
        .ok_or_else(|| ProofError::new("bipersistence node count overflows"))?;
    if node_count > limits.proof.max_nodes || claim.nodes.len() != node_count {
        return Err(ProofError::new(
            "bipersistence node count differs from its grid",
        ));
    }
    Ok((scales, minimum_degrees))
}

pub(super) fn build_nodes(
    claim: &Claim,
    limits: BipersistenceProofLimits,
    scales: &[f64],
    minimum_degrees: &[usize],
    degrees: &[Vec<usize>],
) -> Result<Vec<CheckedNode>, ProofError> {
    let context = NodeBuildContext {
        claim,
        limits,
        scales,
        degrees,
    };
    let mut nodes = Vec::with_capacity(scales.len() * minimum_degrees.len());
    for scale in 0..scales.len() {
        for (density, &minimum_degree) in minimum_degrees.iter().enumerate() {
            nodes.push(build_node(
                &context,
                super::claims::Grade { scale, density },
                minimum_degree,
                nodes.len(),
            )?);
        }
    }
    Ok(nodes)
}

struct NodeBuildContext<'a> {
    claim: &'a Claim,
    limits: BipersistenceProofLimits,
    scales: &'a [f64],
    degrees: &'a [Vec<usize>],
}

fn build_node(
    context: &NodeBuildContext<'_>,
    grade: super::claims::Grade,
    minimum_degree: usize,
    position: usize,
) -> Result<CheckedNode, ProofError> {
    let active = active_vertices(&context.degrees[grade.scale], minimum_degree);
    let edges = filtered_edges(&context.claim.edges, context.scales[grade.scale], &active);
    let space = Space::build(
        context.claim.vertex_count,
        1,
        &edges,
        context.claim.modulus,
        context.limits.proof,
    )?;
    let space_id = space.id(
        context.claim.vertex_count,
        1,
        0.0,
        context.claim.modulus,
        &edges,
    );
    let expected = NodeClaim {
        grade,
        space: space_id,
        rank: space.rank(),
    };
    if context.claim.nodes[position] != expected {
        return Err(ProofError::new(
            "a bipersistence node differs from exact cohomology replay",
        ));
    }
    Ok(CheckedNode {
        space,
        space_id,
        edges,
    })
}

fn active_vertices(degrees: &[usize], minimum_degree: usize) -> Vec<bool> {
    degrees
        .iter()
        .map(|&degree| degree >= minimum_degree)
        .collect()
}

fn filtered_edges(edges: &[WeightedEdge], scale: f64, active: &[bool]) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| f64::from_bits(edge.value_bits) <= scale && active[edge.u] && active[edge.v])
        .map(|edge| Edge {
            u: edge.u,
            v: edge.v,
        })
        .collect()
}
