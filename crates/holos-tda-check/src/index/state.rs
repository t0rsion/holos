use std::collections::{BTreeMap, BTreeSet};

use crate::relative::{IndexLeafContext, decode_verified, verify_index_leaf};
use crate::{ProofBar, ProofError, ProofLimits, canonicalize_diagram, diagrams_equal};

use super::delta::{
    check_delta_nodes, prepare_delta_candidate, validate_delta_context, validate_delta_paths,
    validate_empty_delta,
};
use super::digest::interface_digest;
use super::graded::check_graded_reduction;
use super::model::{
    IndexProofState, InterfaceMode, InterfaceProof, VerifiedIndexDelta, VerifiedIndexSnapshot,
};
use super::scope::{
    CheckCounts, check_disjoint_separator, has_graded_columns, has_zero_cone_center,
    induced_edge_positions, local_graph, remove_duplicate_h0, require_separator, resolve_children,
    validate_child_edge_cover, validate_child_intersections, validate_child_protection,
    validate_child_scopes, validate_interface_scope, validate_relative_artifact,
    verify_parent_relation,
};
use super::wire::{decode_delta, decode_snapshot};

impl IndexProofState {
    /// Decode and verify one complete `HOLOSIP` snapshot.
    pub fn verify_snapshot(
        bytes: &[u8],
        limits: ProofLimits,
    ) -> Result<(Self, VerifiedIndexSnapshot), ProofError> {
        let snapshot = decode_snapshot(bytes, limits)?;
        let nodes = index_nodes(snapshot.nodes)?;
        let state = Self {
            max_dim: snapshot.max_dim,
            modulus: snapshot.modulus,
            threshold: snapshot.threshold,
            graph: snapshot.graph,
            nodes,
            root: snapshot.root,
            diagram: snapshot.diagram,
        };
        let mut checked = BTreeSet::new();
        let mut visiting = BTreeSet::new();
        let mut counts = CheckCounts::default();
        state.verify_subtree(
            state.root,
            &mut checked,
            &mut visiting,
            None,
            limits,
            &mut counts,
        )?;
        if checked.len() != state.nodes.len() {
            return Err(ProofError::new(
                "snapshot contains an interface node outside the root tree",
            ));
        }
        let root = state.nodes[&state.root].clone();
        if root.vertices != (0..state.graph.vertex_count).collect::<Vec<_>>()
            || root.edge_positions != (0..state.graph.edges.len()).collect::<Vec<_>>()
        {
            return Err(ProofError::new(
                "snapshot root does not cover the complete graph envelope",
            ));
        }
        if !root.protected_vertices.is_empty() {
            return Err(ProofError::new(
                "snapshot root has nonempty protected vertices",
            ));
        }
        let root_diagram = state.check_node(&root, limits)?.0;
        if !diagrams_equal(&root_diagram, &state.diagram) {
            return Err(ProofError::new(
                "snapshot diagram differs from the checked root interface",
            ));
        }
        Ok((
            state,
            VerifiedIndexSnapshot {
                root: snapshot.root,
                nodes_checked: counts.nodes,
                composed_nodes_checked: counts.composed_nodes,
                relative_nodes_checked: counts.relative_nodes,
                edge_columns_checked: counts.edge_columns,
                triangle_columns_checked: counts.triangle_columns,
                higher_columns_checked: counts.higher_columns,
            },
        ))
    }

    /// Decode, verify, and atomically apply one `HOLOSDP` delta.
    pub fn apply_delta(
        &mut self,
        bytes: &[u8],
        limits: ProofLimits,
    ) -> Result<VerifiedIndexDelta, ProofError> {
        let delta = decode_delta(bytes, limits)?;
        validate_delta_context(self, &delta)?;
        let (mut candidate, supplied) = prepare_delta_candidate(self, &delta, limits)?;
        validate_delta_paths(&candidate, &delta, &supplied)?;
        let (counts, reused_child_references) =
            check_delta_nodes(&candidate, &delta, &supplied, limits)?;
        validate_empty_delta(self, &delta, &supplied)?;
        candidate.root = delta.new_root;
        candidate.diagram = delta.diagram.clone();
        *self = candidate;
        Ok(VerifiedIndexDelta {
            old_root: delta.old_root,
            new_root: delta.new_root,
            edge_changes: delta.edge_changes.len(),
            nodes_checked: counts.nodes,
            composed_nodes_checked: counts.composed_nodes,
            relative_nodes_checked: counts.relative_nodes,
            reused_child_references,
            edge_columns_checked: counts.edge_columns,
            triangle_columns_checked: counts.triangle_columns,
            higher_columns_checked: counts.higher_columns,
        })
    }

    /// Current verified root content identifier.
    pub fn root(&self) -> &[u8; 32] {
        &self.root
    }

    /// Highest homology dimension checked in this stream.
    pub fn max_dim(&self) -> usize {
        self.max_dim
    }

    /// Current verified exact diagram.
    pub fn diagram(&self) -> &[ProofBar] {
        &self.diagram
    }

    /// Interface nodes retained across the verified stream.
    pub fn retained_nodes(&self) -> usize {
        self.nodes.len()
    }

    fn verify_subtree(
        &self,
        digest: [u8; 32],
        checked: &mut BTreeSet<[u8; 32]>,
        visiting: &mut BTreeSet<[u8; 32]>,
        parent: Option<[u8; 32]>,
        limits: ProofLimits,
        counts: &mut CheckCounts,
    ) -> Result<(), ProofError> {
        let mut stack = vec![(digest, parent, false)];
        while let Some((digest, parent, finishing)) = stack.pop() {
            if finishing {
                let node = self.nodes.get(&digest).ok_or_else(|| {
                    ProofError::new("snapshot references an unknown interface node")
                })?;
                self.finish_subtree(digest, node, checked, visiting, limits, counts)?;
                continue;
            }
            let node = self.enter_subtree(digest, checked, visiting, parent)?;
            stack.push((digest, parent, true));
            for &child in node.children.iter().rev() {
                stack.push((child, Some(digest), false));
            }
        }
        Ok(())
    }

    fn enter_subtree<'a>(
        &'a self,
        digest: [u8; 32],
        checked: &BTreeSet<[u8; 32]>,
        visiting: &mut BTreeSet<[u8; 32]>,
        parent: Option<[u8; 32]>,
    ) -> Result<&'a InterfaceProof, ProofError> {
        if checked.contains(&digest) {
            return Err(ProofError::new(
                "snapshot interface tree contains a repeated child",
            ));
        }
        if !visiting.insert(digest) {
            return Err(ProofError::new("snapshot interface tree contains a cycle"));
        }
        let node = self
            .nodes
            .get(&digest)
            .ok_or_else(|| ProofError::new("snapshot references an unknown interface node"))?;
        self.check_direct_shape(node)?;
        verify_parent_relation(node, parent.map(|digest| &self.nodes[&digest]))?;
        Ok(node)
    }

    fn finish_subtree(
        &self,
        digest: [u8; 32],
        node: &InterfaceProof,
        checked: &mut BTreeSet<[u8; 32]>,
        visiting: &mut BTreeSet<[u8; 32]>,
        limits: ProofLimits,
        counts: &mut CheckCounts,
    ) -> Result<(), ProofError> {
        let (_, computed) = self.check_node(node, limits)?;
        if computed != node.digest {
            return Err(ProofError::new(
                "snapshot interface digest differs from its checked content",
            ));
        }
        counts.record(node, limits)?;
        visiting.remove(&digest);
        checked.insert(digest);
        Ok(())
    }

    pub(super) fn check_direct_shape(&self, node: &InterfaceProof) -> Result<(), ProofError> {
        validate_interface_scope(node, &self.graph)?;
        validate_relative_artifact(node)?;
        let expected_edges = induced_edge_positions(&self.graph, &node.vertices);
        if expected_edges != node.edge_positions {
            return Err(ProofError::new(
                "interface does not contain its complete induced edge scope",
            ));
        }
        if node.children.is_empty() {
            if !node.separator.is_empty() {
                return Err(ProofError::new("a leaf declares a separator"));
            }
            return Ok(());
        }
        let children = resolve_children(node, &self.nodes)?;
        validate_child_protection(node, &children)?;
        validate_child_scopes(node, &children)?;
        validate_child_intersections(node, &children)?;
        validate_child_edge_cover(node, &children)
    }

    pub(super) fn check_node(
        &self,
        node: &InterfaceProof,
        limits: ProofLimits,
    ) -> Result<(Vec<ProofBar>, [u8; 32]), ProofError> {
        let diagram = match node.mode {
            InterfaceMode::Relative => self.check_relative_node(node, limits)?,
            InterfaceMode::Materialized => self.check_materialized_node(node)?,
            InterfaceMode::Disjoint | InterfaceMode::ZeroSimplex | InterfaceMode::ZeroCone => {
                self.check_composed_node(node)?
            }
        };
        let digest = interface_digest(node, &self.graph, self.threshold, &diagram, limits)?;
        Ok((diagram, digest))
    }

    fn check_relative_node(
        &self,
        node: &InterfaceProof,
        limits: ProofLimits,
    ) -> Result<Vec<ProofBar>, ProofError> {
        if has_graded_columns(node) {
            return Err(ProofError::new(
                "a relative interface contains outer reduction columns",
            ));
        }
        let certificate = if node.children.is_empty() {
            verify_index_leaf(
                &node.relative_artifact,
                IndexLeafContext {
                    graph: &self.graph,
                    labels: &node.vertices,
                    threshold: self.threshold,
                    max_dim: self.max_dim,
                    modulus: self.modulus,
                    protected_vertices: &node.protected_vertices,
                    limits,
                },
            )?
        } else {
            self.check_relative_parent(node, limits)?
        };
        if certificate.max_dim != self.max_dim
            || certificate.modulus != self.modulus
            || certificate.protected_vertices != node.protected_vertices
            || !diagrams_equal(&certificate.diagram, &node.diagram)
        {
            return Err(ProofError::new(
                "relative interface differs from the index parameters or diagram",
            ));
        }
        Ok(certificate.diagram)
    }

    fn check_relative_parent(
        &self,
        node: &InterfaceProof,
        limits: ProofLimits,
    ) -> Result<crate::relative::VerifiedCertificate, ProofError> {
        let children = node
            .children
            .iter()
            .map(|child| {
                let child = &self.nodes[child];
                if child.mode != InterfaceMode::Relative {
                    return Err(ProofError::new(
                        "a relative parent references a nonrelative child",
                    ));
                }
                Ok(child.relative_artifact.as_slice())
            })
            .collect::<Result<Vec<_>, _>>()?;
        crate::relative::verify_relative_composition(
            &node.relative_artifact,
            &children,
            &node.protected_vertices,
            limits,
        )?;
        decode_verified(&node.relative_artifact, limits)
    }

    fn check_materialized_node(&self, node: &InterfaceProof) -> Result<Vec<ProofBar>, ProofError> {
        let local = local_graph(&self.graph, node)?;
        let checked = check_graded_reduction(
            &local,
            self.threshold,
            self.modulus,
            self.max_dim,
            &node.graded_columns,
        )?;
        if diagrams_equal(&checked.diagram, &node.diagram) {
            Ok(checked.diagram)
        } else {
            Err(ProofError::new(
                "materialized interface diagram differs from its reduction",
            ))
        }
    }

    fn check_composed_node(&self, node: &InterfaceProof) -> Result<Vec<ProofBar>, ProofError> {
        if has_graded_columns(node) {
            return Err(ProofError::new(
                "a composed interface contains reduction columns",
            ));
        }
        self.check_composition_rule(node)?;
        let mut diagram = node
            .children
            .iter()
            .flat_map(|child| self.nodes[child].diagram.iter().cloned())
            .collect::<Vec<_>>();
        if matches!(
            node.mode,
            InterfaceMode::ZeroSimplex | InterfaceMode::ZeroCone
        ) {
            remove_duplicate_h0(&mut diagram, node.children.len())?;
        }
        canonicalize_diagram(&mut diagram);
        if diagrams_equal(&diagram, &node.diagram) {
            Ok(diagram)
        } else {
            Err(ProofError::new(
                "composed interface diagram differs from its child diagrams",
            ))
        }
    }

    fn check_composition_rule(&self, node: &InterfaceProof) -> Result<(), ProofError> {
        match node.mode {
            InterfaceMode::Relative => Ok(()),
            InterfaceMode::Materialized => Ok(()),
            InterfaceMode::Disjoint => check_disjoint_separator(node),
            InterfaceMode::ZeroSimplex => {
                require_separator(node, "zero-simplex")?;
                self.check_zero_simplex(node)
            }
            InterfaceMode::ZeroCone => {
                require_separator(node, "zero-cone")?;
                self.check_zero_cone(node)
            }
        }
    }

    fn check_zero_simplex(&self, node: &InterfaceProof) -> Result<(), ProofError> {
        let threshold = self.threshold.unwrap_or(f64::INFINITY);
        for (index, &u) in node.separator.iter().enumerate() {
            for &v in &node.separator[index + 1..] {
                let edge = self
                    .graph
                    .edges
                    .binary_search_by_key(&(u, v), |edge| (edge.u, edge.v))
                    .ok()
                    .map(|position| self.graph.edges[position])
                    .ok_or_else(|| {
                        ProofError::new("a zero-simplex separator omits a required edge")
                    })?;
                if edge.value != 0.0 || edge.value > threshold {
                    return Err(ProofError::new(
                        "a zero-simplex separator edge does not enter at zero",
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_zero_cone(&self, node: &InterfaceProof) -> Result<(), ProofError> {
        let threshold = self.threshold.unwrap_or(f64::INFINITY);
        for (index, &u) in node.separator.iter().enumerate() {
            for &v in &node.separator[index + 1..] {
                let value = self.graph.get(u, v);
                if value.is_finite() && value <= threshold && value != 0.0 {
                    return Err(ProofError::new(
                        "a zero-cone separator edge enters above zero",
                    ));
                }
            }
        }
        if has_zero_cone_center(&self.graph, &node.separator, threshold) {
            Ok(())
        } else {
            Err(ProofError::new(
                "a zero-cone separator has no zero-filtration cone vertex",
            ))
        }
    }

    pub(super) fn collect_new_reachable(
        &self,
        digest: [u8; 32],
        supplied: &BTreeSet<[u8; 32]>,
        reached: &mut BTreeSet<[u8; 32]>,
    ) -> Result<(), ProofError> {
        let mut stack = vec![(digest, false)];
        let mut visiting = BTreeSet::new();
        while let Some((digest, finishing)) = stack.pop() {
            if !supplied.contains(&digest) {
                continue;
            }
            if finishing {
                visiting.remove(&digest);
                continue;
            }
            if !visiting.insert(digest) {
                return Err(ProofError::new("delta interface paths contain a cycle"));
            }
            if !reached.insert(digest) {
                visiting.remove(&digest);
                continue;
            }
            let node = self
                .nodes
                .get(&digest)
                .ok_or_else(|| ProofError::new("delta references an unknown interface node"))?;
            stack.push((digest, true));
            for &child in node.children.iter().rev() {
                stack.push((child, false));
            }
        }
        Ok(())
    }

    pub(super) fn require_changed_path(
        &self,
        digest: [u8; 32],
        edge_position: usize,
        supplied: &BTreeSet<[u8; 32]>,
    ) -> Result<(), ProofError> {
        let mut stack = vec![(digest, false)];
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        while let Some((digest, finishing)) = stack.pop() {
            if finishing {
                visiting.remove(&digest);
                visited.insert(digest);
                continue;
            }
            if visited.contains(&digest) {
                continue;
            }
            if !visiting.insert(digest) {
                return Err(ProofError::new("delta changed paths contain a cycle"));
            }
            let node = self
                .nodes
                .get(&digest)
                .ok_or_else(|| ProofError::new("delta references an unknown interface node"))?;
            if node.edge_positions.binary_search(&edge_position).is_err() {
                visiting.remove(&digest);
                continue;
            }
            if !supplied.contains(&digest) {
                return Err(ProofError::new(
                    "delta reuses an interface whose edge value changed",
                ));
            }
            stack.push((digest, true));
            for &child in node.children.iter().rev() {
                stack.push((child, false));
            }
        }
        Ok(())
    }
}

fn index_nodes(
    nodes: Vec<InterfaceProof>,
) -> Result<BTreeMap<[u8; 32], InterfaceProof>, ProofError> {
    let count = nodes.len();
    let indexed: BTreeMap<_, _> = nodes.into_iter().map(|node| (node.digest, node)).collect();
    if indexed.len() != count {
        return Err(ProofError::new("snapshot repeats an interface-node digest"));
    }
    Ok(indexed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Graph;

    fn node(
        digest: [u8; 32],
        children: Vec<[u8; 32]>,
        edge_positions: Vec<usize>,
    ) -> InterfaceProof {
        InterfaceProof {
            digest,
            vertices: vec![0],
            edge_positions,
            separator: Vec::new(),
            protected_vertices: Vec::new(),
            children,
            mode: InterfaceMode::Materialized,
            graded_columns: vec![Vec::new()],
            relative_artifact: Vec::new(),
            diagram: Vec::new(),
        }
    }

    fn state(nodes: Vec<InterfaceProof>) -> IndexProofState {
        IndexProofState {
            max_dim: 0,
            modulus: 2,
            threshold: None,
            graph: Graph::new(1, &[]).unwrap(),
            nodes: nodes.into_iter().map(|node| (node.digest, node)).collect(),
            root: [0; 32],
            diagram: Vec::new(),
        }
    }

    #[test]
    fn duplicate_snapshot_digests_are_rejected_before_indexing() {
        let first = node([1; 32], Vec::new(), Vec::new());
        let error = index_nodes(vec![first.clone(), first]).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("repeats an interface-node digest")
        );
    }

    #[test]
    fn delta_reachability_rejects_cycles_without_recursion() {
        let a = [1; 32];
        let b = [2; 32];
        let state = state(vec![node(a, vec![b], vec![0]), node(b, vec![a], vec![0])]);
        let supplied = BTreeSet::from([a, b]);
        let mut reached = BTreeSet::new();
        let error = state
            .collect_new_reachable(a, &supplied, &mut reached)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("delta interface paths contain a cycle")
        );
    }

    #[test]
    fn changed_path_rejects_cycles_without_recursion() {
        let a = [1; 32];
        let b = [2; 32];
        let state = state(vec![node(a, vec![b], vec![0]), node(b, vec![a], vec![0])]);
        let supplied = BTreeSet::from([a, b]);
        let error = state.require_changed_path(a, 0, &supplied).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("delta changed paths contain a cycle")
        );
    }
}
