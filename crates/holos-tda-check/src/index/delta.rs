use std::collections::BTreeSet;

use crate::{ProofBar, ProofError, ProofLimits, diagrams_equal};

use super::model::{Delta, IndexProofState, InterfaceProof};
use super::scope::CheckCounts;

pub(super) fn validate_delta_context(
    state: &IndexProofState,
    delta: &Delta,
) -> Result<(), ProofError> {
    if delta.max_dim != state.max_dim
        || delta.modulus != state.modulus
        || delta.threshold.map(f64::to_bits) != state.threshold.map(f64::to_bits)
        || delta.vertex_count != state.graph.vertex_count
        || delta.edge_count != state.graph.edges.len()
    {
        return Err(ProofError::new(
            "delta field, threshold, or graph envelope differs from the verified state",
        ));
    }
    if delta.old_root != state.root {
        return Err(ProofError::new(
            "delta old root differs from the verified state",
        ));
    }
    Ok(())
}

pub(super) fn prepare_delta_candidate(
    state: &IndexProofState,
    delta: &Delta,
    limits: ProofLimits,
) -> Result<(IndexProofState, BTreeSet<[u8; 32]>), ProofError> {
    let mut candidate = state.clone();
    for &(position, value) in &delta.edge_changes {
        candidate.graph.edges[position].value = value;
    }
    let supplied = delta
        .nodes
        .iter()
        .map(|node| node.digest)
        .collect::<BTreeSet<_>>();
    validate_supplied_nodes(&candidate, delta, &supplied, limits)?;
    for node in &delta.nodes {
        candidate.nodes.insert(node.digest, node.clone());
    }
    if !candidate.nodes.contains_key(&delta.new_root) {
        return Err(ProofError::new("delta references an unknown new root"));
    }
    Ok((candidate, supplied))
}

pub(super) fn validate_supplied_nodes(
    candidate: &IndexProofState,
    delta: &Delta,
    supplied: &BTreeSet<[u8; 32]>,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    if supplied.len() != delta.nodes.len() {
        return Err(ProofError::new("delta repeats an interface-node digest"));
    }
    let new_nodes = delta
        .nodes
        .iter()
        .filter(|node| !candidate.nodes.contains_key(&node.digest))
        .count();
    if candidate.nodes.len().saturating_add(new_nodes) > limits.max_nodes {
        return Err(ProofError::new("delta exceeds the retained node limit"));
    }
    for node in &delta.nodes {
        if candidate
            .nodes
            .get(&node.digest)
            .is_some_and(|existing| existing != node)
        {
            return Err(ProofError::new(
                "delta changes content under a known interface digest",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_delta_paths(
    candidate: &IndexProofState,
    delta: &Delta,
    supplied: &BTreeSet<[u8; 32]>,
) -> Result<(), ProofError> {
    let mut reached = BTreeSet::new();
    candidate.collect_new_reachable(delta.new_root, supplied, &mut reached)?;
    if reached != *supplied {
        return Err(ProofError::new(
            "delta contains a new interface node outside the new root paths",
        ));
    }
    for &(position, _) in &delta.edge_changes {
        candidate.require_changed_path(delta.new_root, position, supplied)?;
    }
    Ok(())
}

pub(super) fn check_delta_nodes(
    candidate: &IndexProofState,
    delta: &Delta,
    supplied: &BTreeSet<[u8; 32]>,
    limits: ProofLimits,
) -> Result<(CheckCounts, usize), ProofError> {
    let mut counts = CheckCounts::default();
    let mut reused = 0usize;
    for digest in supplied {
        let node = &candidate.nodes[digest];
        candidate.check_direct_shape(node)?;
        reused += node
            .children
            .iter()
            .filter(|child| !supplied.contains(*child))
            .count();
        let (diagram, computed) = candidate.check_node(node, limits)?;
        verify_delta_node(delta, node, &diagram, computed)?;
        counts.record(node, limits)?;
    }
    Ok((counts, reused))
}

pub(super) fn verify_delta_node(
    delta: &Delta,
    node: &InterfaceProof,
    diagram: &[ProofBar],
    computed: [u8; 32],
) -> Result<(), ProofError> {
    if computed != node.digest {
        return Err(ProofError::new(
            "delta interface digest differs from its checked content",
        ));
    }
    if node.digest == delta.new_root && !diagrams_equal(diagram, &delta.diagram) {
        return Err(ProofError::new(
            "delta diagram differs from the checked new root interface",
        ));
    }
    Ok(())
}

pub(super) fn validate_empty_delta(
    state: &IndexProofState,
    delta: &Delta,
    supplied: &BTreeSet<[u8; 32]>,
) -> Result<(), ProofError> {
    if !supplied.is_empty() {
        return Ok(());
    }
    if !delta.edge_changes.is_empty() || delta.new_root != state.root {
        return Err(ProofError::new(
            "a state-changing delta contains no new interface nodes",
        ));
    }
    if !diagrams_equal(&state.diagram, &delta.diagram) {
        return Err(ProofError::new(
            "an empty delta changes the declared diagram",
        ));
    }
    Ok(())
}
