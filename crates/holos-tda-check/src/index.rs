use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::relative::{IndexLeafContext, decode_verified, verify_index_leaf};
use super::{
    Graph, MODULUS_LIMIT, ProofBar, ProofColumn, ProofEdge, ProofError, ProofLimits, ProofTerm,
    SparseColumn, canonicalize_diagram, check_column, check_matrix, checked_threshold,
    diagrams_equal, is_prime,
};

const SNAPSHOT_MAGIC: &[u8; 8] = b"HOLOSIP\0";
const DELTA_MAGIC: &[u8; 8] = b"HOLOSDP\0";
const VERSION: u16 = 4;
const F64_BITS_CODEC: u8 = 1;

/// Whether bytes start with the versioned-index snapshot magic.
pub fn is_index_snapshot(bytes: &[u8]) -> bool {
    bytes.starts_with(SNAPSHOT_MAGIC)
}

/// Counts derived while checking a complete index snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedIndexSnapshot {
    /// Checked root content identifier.
    pub root: [u8; 32],
    /// Interface nodes checked from algebraic reductions.
    pub nodes_checked: usize,
    /// Interface nodes checked by separator composition.
    pub composed_nodes_checked: usize,
    /// Interface nodes checked as relative filtered cores.
    pub relative_nodes_checked: usize,
    /// Edge-boundary columns checked.
    pub edge_columns_checked: usize,
    /// Triangle-boundary columns checked.
    pub triangle_columns_checked: usize,
    /// Boundary columns checked above the triangle dimension.
    pub higher_columns_checked: usize,
}

/// Counts derived while checking one warm index delta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedIndexDelta {
    /// Root required before the delta was applied.
    pub old_root: [u8; 32],
    /// Root established by the delta.
    pub new_root: [u8; 32],
    /// Edge values changed by the delta.
    pub edge_changes: usize,
    /// New interface nodes checked from algebraic reductions.
    pub nodes_checked: usize,
    /// New interface nodes checked by separator composition.
    pub composed_nodes_checked: usize,
    /// New interface nodes checked as relative filtered cores.
    pub relative_nodes_checked: usize,
    /// References from new nodes to already checked child nodes.
    pub reused_child_references: usize,
    /// Edge-boundary columns checked.
    pub edge_columns_checked: usize,
    /// Triangle-boundary columns checked.
    pub triangle_columns_checked: usize,
    /// Boundary columns checked above the triangle dimension.
    pub higher_columns_checked: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct InterfaceProof {
    digest: [u8; 32],
    vertices: Vec<usize>,
    edge_positions: Vec<usize>,
    separator: Vec<usize>,
    protected_vertices: Vec<usize>,
    children: Vec<[u8; 32]>,
    mode: InterfaceMode,
    graded_columns: Vec<Vec<ProofColumn>>,
    relative_artifact: Vec<u8>,
    diagram: Vec<ProofBar>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterfaceMode {
    Relative,
    Materialized,
    Disjoint,
    ZeroSimplex,
    ZeroCone,
}

#[derive(Debug)]
struct Snapshot {
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    graph: Graph,
    root: [u8; 32],
    nodes: Vec<InterfaceProof>,
    diagram: Vec<ProofBar>,
}

#[derive(Debug)]
struct Delta {
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    vertex_count: usize,
    edge_count: usize,
    old_root: [u8; 32],
    new_root: [u8; 32],
    edge_changes: Vec<(usize, f64)>,
    nodes: Vec<InterfaceProof>,
    diagram: Vec<ProofBar>,
}

/// Stateful verifier for one versioned persistence-index envelope.
///
/// Construct it from a complete snapshot. Each accepted delta advances the
/// trusted root atomically and retains old interface nodes for later reuse.
#[derive(Debug, Clone)]
pub struct IndexProofState {
    max_dim: usize,
    modulus: u32,
    threshold: Option<f64>,
    graph: Graph,
    nodes: BTreeMap<[u8; 32], InterfaceProof>,
    root: [u8; 32],
    diagram: Vec<ProofBar>,
}

impl IndexProofState {
    /// Decode and verify one complete `HOLOSIP` snapshot.
    pub fn verify_snapshot(
        bytes: &[u8],
        limits: ProofLimits,
    ) -> Result<(Self, VerifiedIndexSnapshot), ProofError> {
        let snapshot = decode_snapshot(bytes, limits)?;
        let nodes: BTreeMap<_, _> = snapshot
            .nodes
            .into_iter()
            .map(|node| (node.digest, node))
            .collect();
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
        if delta.max_dim != self.max_dim
            || delta.modulus != self.modulus
            || delta.threshold.map(f64::to_bits) != self.threshold.map(f64::to_bits)
            || delta.vertex_count != self.graph.vertex_count
            || delta.edge_count != self.graph.edges.len()
        {
            return Err(ProofError::new(
                "delta field, threshold, or graph envelope differs from the verified state",
            ));
        }
        if delta.old_root != self.root {
            return Err(ProofError::new(
                "delta old root differs from the verified state",
            ));
        }
        let mut candidate = self.clone();
        for &(position, value) in &delta.edge_changes {
            candidate.graph.edges[position].value = value;
        }
        let supplied: BTreeSet<_> = delta.nodes.iter().map(|node| node.digest).collect();
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
            if let Some(existing) = candidate.nodes.get(&node.digest) {
                if existing != node {
                    return Err(ProofError::new(
                        "delta changes content under a known interface digest",
                    ));
                }
            }
        }
        for node in delta.nodes {
            candidate.nodes.insert(node.digest, node);
        }
        if !candidate.nodes.contains_key(&delta.new_root) {
            return Err(ProofError::new("delta references an unknown new root"));
        }

        let mut reached_new = BTreeSet::new();
        candidate.collect_new_reachable(delta.new_root, &supplied, &mut reached_new)?;
        if reached_new != supplied {
            return Err(ProofError::new(
                "delta contains a new interface node outside the new root paths",
            ));
        }
        for &(position, _) in &delta.edge_changes {
            candidate.require_changed_path(delta.new_root, position, &supplied)?;
        }

        let mut counts = CheckCounts::default();
        let mut reused_child_references = 0usize;
        for digest in &supplied {
            let node = candidate.nodes[digest].clone();
            candidate.check_direct_shape(&node)?;
            for child in &node.children {
                if !supplied.contains(child) {
                    reused_child_references += 1;
                }
            }
            let (diagram, computed) = candidate.check_node(&node, limits)?;
            if computed != node.digest {
                return Err(ProofError::new(
                    "delta interface digest differs from its checked content",
                ));
            }
            counts.nodes += 1;
            if node.mode != InterfaceMode::Materialized {
                counts.composed_nodes += 1;
            }
            if node.mode == InterfaceMode::Relative {
                counts.relative_nodes += 1;
            }
            counts.add_node(&node, limits)?;
            if node.digest == delta.new_root && !diagrams_equal(&diagram, &delta.diagram) {
                return Err(ProofError::new(
                    "delta diagram differs from the checked new root interface",
                ));
            }
        }
        if supplied.is_empty() {
            if !delta.edge_changes.is_empty() || delta.new_root != self.root {
                return Err(ProofError::new(
                    "a state-changing delta contains no new interface nodes",
                ));
            }
            if !diagrams_equal(&self.diagram, &delta.diagram) {
                return Err(ProofError::new(
                    "an empty delta changes the declared diagram",
                ));
            }
        }
        candidate.root = delta.new_root;
        candidate.diagram = delta.diagram;
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
        if let Some(parent) = parent {
            let parent = &self.nodes[&parent];
            if node.vertices.len() >= parent.vertices.len() {
                return Err(ProofError::new(
                    "snapshot child scope is not smaller than its parent",
                ));
            }
            if node.protected_vertices
                != inherited_protection(
                    &parent.protected_vertices,
                    &parent.separator,
                    &node.vertices,
                )
            {
                return Err(ProofError::new(
                    "snapshot child protects the wrong ancestor separators",
                ));
            }
        } else if !node.protected_vertices.is_empty() {
            return Err(ProofError::new(
                "snapshot root has nonempty protected vertices",
            ));
        }
        for &child in &node.children {
            self.verify_subtree(child, checked, visiting, Some(digest), limits, counts)?;
        }
        let (_, computed) = self.check_node(node, limits)?;
        if computed != node.digest {
            return Err(ProofError::new(
                "snapshot interface digest differs from its checked content",
            ));
        }
        counts.nodes += 1;
        if node.mode != InterfaceMode::Materialized {
            counts.composed_nodes += 1;
        }
        if node.mode == InterfaceMode::Relative {
            counts.relative_nodes += 1;
        }
        counts.add_node(node, limits)?;
        visiting.remove(&digest);
        checked.insert(digest);
        Ok(())
    }

    fn check_direct_shape(&self, node: &InterfaceProof) -> Result<(), ProofError> {
        if node.vertices.is_empty()
            || !node.vertices.windows(2).all(|pair| pair[0] < pair[1])
            || node
                .vertices
                .iter()
                .any(|&vertex| vertex >= self.graph.vertex_count)
            || !node.edge_positions.windows(2).all(|pair| pair[0] < pair[1])
            || node
                .edge_positions
                .iter()
                .any(|&position| position >= self.graph.edges.len())
            || !node.separator.windows(2).all(|pair| pair[0] < pair[1])
            || node
                .separator
                .iter()
                .any(|vertex| node.vertices.binary_search(vertex).is_err())
            || !node
                .protected_vertices
                .windows(2)
                .all(|pair| pair[0] < pair[1])
            || node
                .protected_vertices
                .iter()
                .any(|vertex| node.vertices.binary_search(vertex).is_err())
        {
            return Err(ProofError::new("interface scope is not canonical"));
        }
        if (node.mode == InterfaceMode::Relative) == node.relative_artifact.is_empty() {
            return Err(ProofError::new(
                "relative interface mode and artifact presence disagree",
            ));
        }
        let expected_edges: Vec<_> = self
            .graph
            .edges
            .iter()
            .enumerate()
            .filter_map(|(position, edge)| {
                (node.vertices.binary_search(&edge.u).is_ok()
                    && node.vertices.binary_search(&edge.v).is_ok())
                .then_some(position)
            })
            .collect();
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
        if node.children.len() < 2 {
            return Err(ProofError::new(
                "an interface split has fewer than two children",
            ));
        }
        let children: Vec<_> = node
            .children
            .iter()
            .map(|digest| {
                self.nodes
                    .get(digest)
                    .ok_or_else(|| ProofError::new("interface references an unknown child"))
            })
            .collect::<Result<_, _>>()?;
        for child in &children {
            let expected =
                inherited_protection(&node.protected_vertices, &node.separator, &child.vertices);
            if child.protected_vertices != expected {
                return Err(ProofError::new(
                    "interface child protects the wrong ancestor separators",
                ));
            }
        }
        let mut union = BTreeSet::new();
        for child in &children {
            if child.vertices.len() >= node.vertices.len()
                || child
                    .vertices
                    .iter()
                    .any(|vertex| node.vertices.binary_search(vertex).is_err())
            {
                return Err(ProofError::new(
                    "interface child is not a proper subscope of its parent",
                ));
            }
            union.extend(child.vertices.iter().copied());
        }
        if union.into_iter().collect::<Vec<_>>() != node.vertices {
            return Err(ProofError::new(
                "interface children do not cover their parent vertices",
            ));
        }
        for left in 0..children.len() {
            for right in left + 1..children.len() {
                let intersection: Vec<_> = children[left]
                    .vertices
                    .iter()
                    .filter(|vertex| children[right].vertices.binary_search(vertex).is_ok())
                    .copied()
                    .collect();
                if intersection != node.separator {
                    return Err(ProofError::new(
                        "interface-child intersection differs from the declared separator",
                    ));
                }
            }
        }
        for &position in &node.edge_positions {
            if !children
                .iter()
                .any(|child| child.edge_positions.binary_search(&position).is_ok())
            {
                return Err(ProofError::new(
                    "interface parent has an edge outside every child",
                ));
            }
        }
        Ok(())
    }

    fn check_node(
        &self,
        node: &InterfaceProof,
        limits: ProofLimits,
    ) -> Result<(Vec<ProofBar>, [u8; 32]), ProofError> {
        let diagram = match node.mode {
            InterfaceMode::Relative => {
                if node
                    .graded_columns
                    .iter()
                    .any(|columns| !columns.is_empty())
                {
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
                    super::relative::verify_relative_composition(
                        &node.relative_artifact,
                        &children,
                        &node.protected_vertices,
                        limits,
                    )?;
                    decode_verified(&node.relative_artifact, limits)?
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
                certificate.diagram
            }
            InterfaceMode::Materialized => {
                let local = local_graph(&self.graph, node)?;
                let checked = check_graded_reduction(
                    &local,
                    self.threshold,
                    self.modulus,
                    self.max_dim,
                    &node.graded_columns,
                )?;
                if !diagrams_equal(&checked.diagram, &node.diagram) {
                    return Err(ProofError::new(
                        "materialized interface diagram differs from its reduction",
                    ));
                }
                checked.diagram
            }
            InterfaceMode::Disjoint | InterfaceMode::ZeroSimplex | InterfaceMode::ZeroCone => {
                if node
                    .graded_columns
                    .iter()
                    .any(|columns| !columns.is_empty())
                {
                    return Err(ProofError::new(
                        "a composed interface contains reduction columns",
                    ));
                }
                self.check_composition_rule(node)?;
                let mut diagram: Vec<_> = node
                    .children
                    .iter()
                    .flat_map(|child| self.nodes[child].diagram.iter().cloned())
                    .collect();
                if matches!(
                    node.mode,
                    InterfaceMode::ZeroSimplex | InterfaceMode::ZeroCone
                ) {
                    for _ in 1..node.children.len() {
                        let position = diagram
                            .iter()
                            .position(|bar| {
                                bar.dimension == 0 && bar.birth == 0.0 && bar.death == f64::INFINITY
                            })
                            .ok_or_else(|| {
                                ProofError::new("contractible composition lacks a child H0 class")
                            })?;
                        diagram.remove(position);
                    }
                }
                canonicalize_diagram(&mut diagram);
                if !diagrams_equal(&diagram, &node.diagram) {
                    return Err(ProofError::new(
                        "composed interface diagram differs from its child diagrams",
                    ));
                }
                diagram
            }
        };
        let digest = interface_digest(node, &self.graph, self.threshold, &diagram, limits)?;
        Ok((diagram, digest))
    }

    fn check_composition_rule(&self, node: &InterfaceProof) -> Result<(), ProofError> {
        match node.mode {
            InterfaceMode::Relative => Ok(()),
            InterfaceMode::Materialized => Ok(()),
            InterfaceMode::Disjoint if node.separator.is_empty() => Ok(()),
            InterfaceMode::Disjoint => Err(ProofError::new(
                "a disjoint composition has a nonempty separator",
            )),
            InterfaceMode::ZeroSimplex if node.separator.is_empty() => Err(ProofError::new(
                "a zero-simplex composition has an empty separator",
            )),
            InterfaceMode::ZeroSimplex => {
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
            InterfaceMode::ZeroCone if node.separator.is_empty() => Err(ProofError::new(
                "a zero-cone composition has an empty separator",
            )),
            InterfaceMode::ZeroCone => {
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
                let has_center = node.separator.iter().any(|&center| {
                    node.separator.iter().all(|&vertex| {
                        center == vertex
                            || (self.graph.get(center, vertex).is_finite()
                                && self.graph.get(center, vertex) == 0.0
                                && self.graph.get(center, vertex) <= threshold)
                    })
                });
                if !has_center {
                    return Err(ProofError::new(
                        "a zero-cone separator has no zero-filtration cone vertex",
                    ));
                }
                Ok(())
            }
        }
    }

    fn collect_new_reachable(
        &self,
        digest: [u8; 32],
        supplied: &BTreeSet<[u8; 32]>,
        reached: &mut BTreeSet<[u8; 32]>,
    ) -> Result<(), ProofError> {
        if !supplied.contains(&digest) || !reached.insert(digest) {
            return Ok(());
        }
        let node = self
            .nodes
            .get(&digest)
            .ok_or_else(|| ProofError::new("delta references an unknown interface node"))?;
        for &child in &node.children {
            self.collect_new_reachable(child, supplied, reached)?;
        }
        Ok(())
    }

    fn require_changed_path(
        &self,
        digest: [u8; 32],
        edge_position: usize,
        supplied: &BTreeSet<[u8; 32]>,
    ) -> Result<(), ProofError> {
        let node = &self.nodes[&digest];
        if node.edge_positions.binary_search(&edge_position).is_err() {
            return Ok(());
        }
        if !supplied.contains(&digest) {
            return Err(ProofError::new(
                "delta reuses an interface whose edge value changed",
            ));
        }
        for &child in &node.children {
            self.require_changed_path(child, edge_position, supplied)?;
        }
        Ok(())
    }
}

fn inherited_protection(
    inherited: &[usize],
    separator: &[usize],
    child_vertices: &[usize],
) -> Vec<usize> {
    let mut protected: Vec<_> = inherited
        .iter()
        .chain(separator)
        .copied()
        .filter(|vertex| child_vertices.binary_search(vertex).is_ok())
        .collect();
    protected.sort_unstable();
    protected.dedup();
    protected
}

#[derive(Default)]
struct CheckCounts {
    nodes: usize,
    composed_nodes: usize,
    relative_nodes: usize,
    edge_columns: usize,
    triangle_columns: usize,
    higher_columns: usize,
}

impl CheckCounts {
    fn add_columns(&mut self, columns: &[Vec<ProofColumn>]) {
        self.edge_columns += columns.first().map_or(0, Vec::len);
        self.triangle_columns += columns.get(1).map_or(0, Vec::len);
        self.higher_columns += columns.iter().skip(2).map(Vec::len).sum::<usize>();
    }

    fn add_node(&mut self, node: &InterfaceProof, limits: ProofLimits) -> Result<(), ProofError> {
        self.add_columns(&node.graded_columns);
        if node.mode == InterfaceMode::Relative {
            let relative = decode_verified(&node.relative_artifact, limits)?;
            self.add_columns(&relative.columns);
        }
        Ok(())
    }
}

fn local_graph(graph: &Graph, node: &InterfaceProof) -> Result<Graph, ProofError> {
    let positions: BTreeMap<_, _> = node
        .vertices
        .iter()
        .copied()
        .enumerate()
        .map(|(local, original)| (original, local))
        .collect();
    let edges: Vec<_> = node
        .edge_positions
        .iter()
        .map(|&position| {
            let edge = graph.edges[position];
            ProofEdge {
                u: positions[&edge.u],
                v: positions[&edge.v],
                value: edge.value,
            }
        })
        .collect();
    Graph::new(node.vertices.len(), &edges)
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct GradedSimplexKey(Vec<usize>);

#[derive(Clone)]
struct GradedSimplex {
    key: GradedSimplexKey,
    value: f64,
}

struct GradedComplex {
    simplices: Vec<Vec<GradedSimplex>>,
    rows: Vec<BTreeMap<GradedSimplexKey, usize>>,
}

impl GradedComplex {
    fn build(
        graph: &Graph,
        threshold: Option<f64>,
        expected_columns: &[Vec<ProofColumn>],
    ) -> Result<Self, ProofError> {
        let threshold = checked_threshold(threshold)?;
        let vertices: Vec<GradedSimplex> = (0..graph.vertex_count)
            .map(|vertex| GradedSimplex {
                key: GradedSimplexKey(vec![vertex]),
                value: 0.0,
            })
            .collect();
        let mut simplices = vec![vertices];
        for (offset, expected) in expected_columns.iter().enumerate() {
            let dimension = offset + 1;
            let mut next = Vec::new();
            for simplex in &simplices[dimension - 1] {
                let start = simplex.key.0.last().copied().unwrap_or(0) + 1;
                for vertex in start..graph.vertex_count {
                    let mut value = simplex.value;
                    let mut clique = true;
                    for &member in &simplex.key.0 {
                        let edge = graph.get(member, vertex);
                        if !edge.is_finite() || edge > threshold {
                            clique = false;
                            break;
                        }
                        value = value.max(edge);
                    }
                    if clique {
                        if next.len() == expected.len() {
                            return Err(ProofError::new(format!(
                                "dimension {dimension} simplex count exceeds the proof"
                            )));
                        }
                        let mut key = simplex.key.0.clone();
                        key.push(vertex);
                        next.push(GradedSimplex {
                            key: GradedSimplexKey(key),
                            value,
                        });
                    }
                }
            }
            if next.len() != expected.len() {
                return Err(ProofError::new(format!(
                    "dimension {dimension} has {} simplices but the proof records {} columns",
                    next.len(),
                    expected.len()
                )));
            }
            next.sort_by(|left, right| {
                left.value
                    .total_cmp(&right.value)
                    .then_with(|| right.key.0.iter().rev().cmp(left.key.0.iter().rev()))
            });
            simplices.push(next);
        }
        let rows = simplices
            .iter()
            .map(|dimension| {
                dimension
                    .iter()
                    .enumerate()
                    .map(|(position, simplex)| (simplex.key.clone(), position))
                    .collect()
            })
            .collect();
        Ok(Self { simplices, rows })
    }

    fn boundaries(&self, dimension: usize, modulus: u32) -> Result<Vec<SparseColumn>, ProofError> {
        let modulus = modulus as u64;
        self.simplices[dimension]
            .iter()
            .map(|simplex| {
                let mut column = SparseColumn::default();
                for removed in 0..simplex.key.0.len() {
                    let mut face = simplex.key.0.clone();
                    face.remove(removed);
                    let row = self.rows[dimension - 1]
                        .get(&GradedSimplexKey(face))
                        .copied()
                        .ok_or_else(|| ProofError::new("simplex boundary omits a face"))?;
                    column.insert(row, if removed % 2 == 0 { 1 } else { modulus - 1 });
                }
                Ok(column)
            })
            .collect()
    }
}

struct CheckedGradedReduction {
    diagram: Vec<ProofBar>,
}

fn check_graded_reduction(
    graph: &Graph,
    threshold: Option<f64>,
    modulus: u32,
    max_dim: usize,
    columns: &[Vec<ProofColumn>],
) -> Result<CheckedGradedReduction, ProofError> {
    if columns.len() != max_dim + 1 {
        return Err(ProofError::new(
            "materialized interface has the wrong boundary count",
        ));
    }
    let complex = GradedComplex::build(graph, threshold, columns)?;
    let mut reduced = Vec::with_capacity(columns.len());
    for dimension in 1..=max_dim + 1 {
        reduced.push(check_matrix(
            &complex.boundaries(dimension, modulus)?,
            &columns[dimension - 1],
            modulus,
            &format!("dimension {dimension}"),
        )?);
    }
    let mut diagram = Vec::new();
    for homology_dimension in 0..=max_dim {
        let births = if homology_dimension == 0 {
            vec![true; graph.vertex_count]
        } else {
            reduced[homology_dimension - 1]
                .iter()
                .map(|column| column.0.is_empty())
                .collect()
        };
        let deaths: BTreeMap<_, _> = reduced[homology_dimension]
            .iter()
            .enumerate()
            .filter_map(|(column, reduction)| reduction.pivot().map(|(row, _)| (row, column)))
            .collect();
        for (birth_position, is_birth) in births.into_iter().enumerate() {
            if !is_birth {
                continue;
            }
            let birth = complex.simplices[homology_dimension][birth_position].value;
            let death = deaths
                .get(&birth_position)
                .map_or(f64::INFINITY, |&position| {
                    complex.simplices[homology_dimension + 1][position].value
                });
            if death > birth {
                diagram.push(ProofBar {
                    dimension: homology_dimension,
                    birth,
                    death,
                });
            }
        }
    }
    canonicalize_diagram(&mut diagram);
    Ok(CheckedGradedReduction { diagram })
}

fn check_graded_diagram(diagram: &[ProofBar], max_dim: usize) -> Result<(), ProofError> {
    for bar in diagram {
        if bar.dimension > max_dim
            || !bar.birth.is_finite()
            || bar.birth < 0.0
            || bar.death.is_nan()
            || bar.death < 0.0
            || bar.death <= bar.birth
        {
            return Err(ProofError::new("graded diagram contains an invalid bar"));
        }
    }
    let mut canonical = diagram.to_vec();
    canonicalize_diagram(&mut canonical);
    if !diagrams_equal(&canonical, diagram) {
        return Err(ProofError::new("graded diagram bars are not canonical"));
    }
    Ok(())
}

fn interface_digest(
    node: &InterfaceProof,
    graph: &Graph,
    threshold: Option<f64>,
    diagram: &[ProofBar],
    limits: ProofLimits,
) -> Result<[u8; 32], ProofError> {
    let mut hash = Sha256::new();
    hash.update(b"holos-filtered-interface-v4");
    digest_usizes(&mut hash, &node.vertices);
    digest_usizes(&mut hash, &node.edge_positions);
    digest_usizes(&mut hash, &node.separator);
    digest_usizes(&mut hash, &node.protected_vertices);
    hash.update((node.children.len() as u64).to_be_bytes());
    for child in &node.children {
        hash.update(child);
    }
    match node.mode {
        InterfaceMode::Relative => {
            let relative = decode_verified(&node.relative_artifact, limits)?;
            hash.update([4]);
            hash.update(relative.source_digest());
            hash.update(relative.digest);
        }
        InterfaceMode::Materialized => {
            hash.update([0]);
            let local = local_graph(graph, node)?;
            hash.update(filtered_graph_digest(&local, threshold));
            hash.update(((node.graded_columns.len() - 1) as u64).to_be_bytes());
            hash.update((node.graded_columns.len() as u64).to_be_bytes());
            for columns in &node.graded_columns {
                digest_columns(&mut hash, columns);
            }
        }
        InterfaceMode::Disjoint => hash.update([1]),
        InterfaceMode::ZeroSimplex => hash.update([2]),
        InterfaceMode::ZeroCone => hash.update([3]),
    }
    hash.update((diagram.len() as u64).to_be_bytes());
    for bar in diagram {
        hash.update((bar.dimension as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
    Ok(hash.finalize().into())
}

fn filtered_graph_digest(graph: &Graph, threshold: Option<f64>) -> [u8; 32] {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    let edges: Vec<_> = graph
        .edges
        .iter()
        .filter(|edge| edge.value <= threshold)
        .collect();
    let mut hash = Sha256::new();
    hash.update(b"holos-certified-graph-v1");
    hash.update((graph.vertex_count as u64).to_be_bytes());
    hash.update((edges.len() as u64).to_be_bytes());
    for edge in edges {
        hash.update((edge.u as u64).to_be_bytes());
        hash.update((edge.v as u64).to_be_bytes());
        hash.update(edge.value.to_bits().to_be_bytes());
    }
    hash.finalize().into()
}

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for &value in values {
        hash.update((value as u64).to_be_bytes());
    }
}

fn digest_columns(hash: &mut Sha256, columns: &[ProofColumn]) {
    hash.update((columns.len() as u64).to_be_bytes());
    for column in columns {
        hash.update((column.terms.len() as u64).to_be_bytes());
        for term in &column.terms {
            hash.update((term.index as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
}

fn decode_snapshot(bytes: &[u8], limits: ProofLimits) -> Result<Snapshot, ProofError> {
    let mut reader = Reader::new(bytes, limits, SNAPSHOT_MAGIC)?;
    let (max_dim, modulus, threshold, vertex_count) = reader.header(limits)?;
    let edge_count = reader.bounded_usize("snapshot edge count", limits.max_edges)?;
    let node_count = reader.bounded_usize("snapshot node count", limits.max_nodes)?;
    let bar_count = reader.bounded_usize("snapshot bar count", limits.max_bars)?;
    let root = reader.array32()?;
    let edges = decode_edges(&mut reader, vertex_count, edge_count)?;
    let graph = Graph::new(vertex_count, &edges)?;
    let mut totals = Totals::default();
    let nodes = decode_nodes(
        &mut reader,
        node_count,
        max_dim,
        modulus,
        limits,
        &mut totals,
    )?;
    let diagram = decode_diagram(&mut reader, bar_count, max_dim)?;
    reader.finish()?;
    Ok(Snapshot {
        max_dim,
        modulus,
        threshold,
        graph,
        root,
        nodes,
        diagram,
    })
}

fn decode_delta(bytes: &[u8], limits: ProofLimits) -> Result<Delta, ProofError> {
    let mut reader = Reader::new(bytes, limits, DELTA_MAGIC)?;
    let (max_dim, modulus, threshold, vertex_count) = reader.header(limits)?;
    let edge_count = reader.bounded_usize("delta edge count", limits.max_edges)?;
    let change_count = reader.bounded_usize("delta edge-change count", limits.max_edges)?;
    let node_count = reader.bounded_usize("delta node count", limits.max_nodes)?;
    let bar_count = reader.bounded_usize("delta bar count", limits.max_bars)?;
    let old_root = reader.array32()?;
    let new_root = reader.array32()?;
    if edge_count == 0 && change_count != 0 {
        return Err(ProofError::new(
            "an empty index cannot contain delta edge changes",
        ));
    }
    let mut edge_changes = Vec::with_capacity(change_count);
    for _ in 0..change_count {
        let position = reader.bounded_usize("delta edge position", edge_count.saturating_sub(1))?;
        let value = f64::from_bits(reader.u64()?);
        if !value.is_finite() || value < 0.0 {
            return Err(ProofError::new(
                "delta edge value is not finite and non-negative",
            ));
        }
        edge_changes.push((position, value));
    }
    if !edge_changes.windows(2).all(|pair| pair[0].0 < pair[1].0) {
        return Err(ProofError::new("delta edge changes are not canonical"));
    }
    let mut totals = Totals::default();
    let nodes = decode_nodes(
        &mut reader,
        node_count,
        max_dim,
        modulus,
        limits,
        &mut totals,
    )?;
    let diagram = decode_diagram(&mut reader, bar_count, max_dim)?;
    reader.finish()?;
    Ok(Delta {
        max_dim,
        modulus,
        threshold,
        vertex_count,
        edge_count,
        old_root,
        new_root,
        edge_changes,
        nodes,
        diagram,
    })
}

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    count: usize,
) -> Result<Vec<ProofEdge>, ProofError> {
    let mut edges = Vec::with_capacity(count);
    for _ in 0..count {
        let edge = ProofEdge {
            u: reader.usize()?,
            v: reader.usize()?,
            value: f64::from_bits(reader.u64()?),
        };
        if edge.u >= edge.v || edge.v >= vertex_count || !edge.value.is_finite() || edge.value < 0.0
        {
            return Err(ProofError::new("snapshot edge is not canonical"));
        }
        edges.push(edge);
    }
    if !edges
        .windows(2)
        .all(|pair| (pair[0].u, pair[0].v) < (pair[1].u, pair[1].v))
    {
        return Err(ProofError::new("snapshot edges are not in canonical order"));
    }
    Ok(edges)
}

#[derive(Default)]
struct Totals {
    vertices: usize,
    edge_positions: usize,
    simplex_columns: usize,
    terms: usize,
}

fn decode_nodes(
    reader: &mut Reader<'_>,
    count: usize,
    max_dim: usize,
    modulus: u32,
    limits: ProofLimits,
    totals: &mut Totals,
) -> Result<Vec<InterfaceProof>, ProofError> {
    let mut nodes = Vec::with_capacity(count);
    for _ in 0..count {
        let digest = reader.array32()?;
        let mode = match reader.u8()? {
            0 => InterfaceMode::Materialized,
            1 => InterfaceMode::Disjoint,
            2 => InterfaceMode::ZeroSimplex,
            3 => InterfaceMode::ZeroCone,
            4 => InterfaceMode::Relative,
            _ => return Err(ProofError::new("invalid interface-mode tag")),
        };
        let vertex_count = reader.usize()?;
        let edge_count = reader.usize()?;
        let separator_count = reader.usize()?;
        let protected_count =
            reader.bounded_usize("protected vertex count", limits.max_vertices)?;
        let child_count = reader.usize()?;
        let boundary_count = reader.usize()?;
        if boundary_count != max_dim + 1 {
            return Err(ProofError::new(
                "interface boundary count differs from the proof dimension",
            ));
        }
        let mut column_counts = Vec::with_capacity(boundary_count);
        for dimension in 1..=boundary_count {
            let limit = if dimension == 1 {
                limits.max_edges
            } else if dimension == 2 {
                limits.max_triangles
            } else {
                limits.max_higher_simplices
            };
            column_counts.push(reader.bounded_usize("interface simplex columns", limit)?);
        }
        let bar_count = reader.bounded_usize("interface bar count", limits.max_bars)?;
        let relative_byte_count =
            reader.bounded_usize("relative interface byte count", limits.max_bytes)?;
        totals.vertices = bounded_sum(
            totals.vertices,
            vertex_count,
            limits.max_vertices.saturating_mul(limits.max_nodes),
            "interface vertices",
        )?;
        totals.edge_positions = bounded_sum(
            totals.edge_positions,
            edge_count,
            limits.max_references,
            "interface edge positions",
        )?;
        for &column_count in &column_counts {
            totals.simplex_columns = bounded_sum(
                totals.simplex_columns,
                column_count,
                limits.max_references,
                "interface simplex columns",
            )?;
        }
        let vertices = decode_usizes(reader, vertex_count)?;
        let edge_positions = decode_usizes(reader, edge_count)?;
        let separator = decode_usizes(reader, separator_count)?;
        let protected_vertices = decode_usizes(reader, protected_count)?;
        let mut children = Vec::with_capacity(child_count);
        for _ in 0..child_count {
            children.push(reader.array32()?);
        }
        let graded_columns = column_counts
            .into_iter()
            .map(|column_count| {
                decode_columns(
                    reader,
                    column_count,
                    modulus,
                    limits.max_terms,
                    &mut totals.terms,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let diagram = decode_diagram(reader, bar_count, max_dim)?;
        let relative_artifact = reader.take(relative_byte_count)?.to_vec();
        nodes.push(InterfaceProof {
            digest,
            vertices,
            edge_positions,
            separator,
            protected_vertices,
            children,
            mode,
            graded_columns,
            relative_artifact,
            diagram,
        });
    }
    Ok(nodes)
}

fn decode_usizes(reader: &mut Reader<'_>, count: usize) -> Result<Vec<usize>, ProofError> {
    let mut output = Vec::with_capacity(count);
    for _ in 0..count {
        output.push(reader.usize()?);
    }
    Ok(output)
}

fn decode_columns(
    reader: &mut Reader<'_>,
    count: usize,
    modulus: u32,
    term_limit: usize,
    total_terms: &mut usize,
) -> Result<Vec<ProofColumn>, ProofError> {
    let mut columns = Vec::with_capacity(count);
    for target in 0..count {
        let term_count = reader.usize()?;
        *total_terms = bounded_sum(*total_terms, term_count, term_limit, "index proof terms")?;
        let mut terms = Vec::with_capacity(term_count);
        for _ in 0..term_count {
            terms.push(ProofTerm {
                index: reader.usize()?,
                coefficient: reader.u32()?,
            });
        }
        check_column(target, &terms, modulus)?;
        columns.push(ProofColumn { terms });
    }
    Ok(columns)
}

fn decode_diagram(
    reader: &mut Reader<'_>,
    count: usize,
    max_dim: usize,
) -> Result<Vec<ProofBar>, ProofError> {
    let mut diagram = Vec::with_capacity(count);
    for _ in 0..count {
        diagram.push(ProofBar {
            dimension: reader.usize()?,
            birth: f64::from_bits(reader.u64()?),
            death: f64::from_bits(reader.u64()?),
        });
    }
    check_graded_diagram(&diagram, max_dim)?;
    canonicalize_diagram(&mut diagram);
    Ok(diagram)
}

fn bounded_sum(
    current: usize,
    added: usize,
    limit: usize,
    label: &str,
) -> Result<usize, ProofError> {
    let total = current
        .checked_add(added)
        .ok_or_else(|| ProofError::new(format!("{label} overflow")))?;
    if total > limit {
        return Err(ProofError::new(format!(
            "{label} count {total} exceeds the limit {limit}"
        )));
    }
    Ok(total)
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8], limits: ProofLimits, magic: &[u8; 8]) -> Result<Self, ProofError> {
        if bytes.len() > limits.max_bytes {
            return Err(ProofError::new(format!(
                "{} bytes exceed the limit {}",
                bytes.len(),
                limits.max_bytes
            )));
        }
        let mut reader = Self { bytes, position: 0 };
        if reader.take(8)? != magic {
            return Err(ProofError::new("wrong index-proof magic bytes"));
        }
        Ok(reader)
    }

    fn header(
        &mut self,
        limits: ProofLimits,
    ) -> Result<(usize, u32, Option<f64>, usize), ProofError> {
        if self.u16()? != VERSION {
            return Err(ProofError::new("unsupported index-proof version"));
        }
        if self.u8()? != F64_BITS_CODEC {
            return Err(ProofError::new("unsupported index-proof scalar codec"));
        }
        let max_dim = self.bounded_usize("index homology dimension", limits.max_dimension)?;
        let modulus = self.u32()?;
        if !is_prime(modulus as u64) || modulus as u64 >= MODULUS_LIMIT {
            return Err(ProofError::new(format!(
                "modulus must be a prime below {MODULUS_LIMIT}, got {modulus}"
            )));
        }
        let threshold = self.optional_f64()?;
        checked_threshold(threshold)?;
        let vertex_count = self.usize()?;
        if vertex_count == 0 {
            return Err(ProofError::new("index proof has no vertices"));
        }
        Ok((max_dim, modulus, threshold, vertex_count))
    }

    fn finish(&self) -> Result<(), ProofError> {
        if self.position != self.bytes.len() {
            return Err(ProofError::new(format!(
                "{} trailing bytes after the index proof",
                self.bytes.len() - self.position
            )));
        }
        Ok(())
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProofError::new("index-proof position overflow"))?;
        if end > self.bytes.len() {
            return Err(ProofError::new("truncated index proof"));
        }
        let output = &self.bytes[self.position..end];
        self.position = end;
        Ok(output)
    }

    fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProofError> {
        let mut bytes = [0; 2];
        bytes.copy_from_slice(self.take(2)?);
        Ok(u16::from_be_bytes(bytes))
    }

    fn u32(&mut self) -> Result<u32, ProofError> {
        let mut bytes = [0; 4];
        bytes.copy_from_slice(self.take(4)?);
        Ok(u32::from_be_bytes(bytes))
    }

    fn u64(&mut self) -> Result<u64, ProofError> {
        let mut bytes = [0; 8];
        bytes.copy_from_slice(self.take(8)?);
        Ok(u64::from_be_bytes(bytes))
    }

    fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProofError::new("index-proof integer does not fit usize"))
    }

    fn bounded_usize(&mut self, label: &str, limit: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > limit {
            return Err(ProofError::new(format!(
                "{label} {value} exceeds the limit {limit}"
            )));
        }
        Ok(value)
    }

    fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        let mut bytes = [0; 32];
        bytes.copy_from_slice(self.take(32)?);
        Ok(bytes)
    }

    fn optional_f64(&mut self) -> Result<Option<f64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(f64::from_bits(self.u64()?))),
            _ => Err(ProofError::new("invalid optional threshold tag")),
        }
    }
}
