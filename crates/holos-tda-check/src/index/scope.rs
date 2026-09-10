use std::collections::{BTreeMap, BTreeSet};

use crate::relative::decode_verified;
use crate::{Graph, ProofBar, ProofColumn, ProofEdge, ProofError, ProofLimits};

use super::model::{InterfaceMode, InterfaceProof};

pub(super) fn has_graded_columns(node: &InterfaceProof) -> bool {
    node.graded_columns
        .iter()
        .any(|columns| !columns.is_empty())
}

pub(super) fn check_disjoint_separator(node: &InterfaceProof) -> Result<(), ProofError> {
    if node.separator.is_empty() {
        Ok(())
    } else {
        Err(ProofError::new(
            "a disjoint composition has a nonempty separator",
        ))
    }
}

pub(super) fn require_separator(node: &InterfaceProof, mode: &str) -> Result<(), ProofError> {
    if node.separator.is_empty() {
        Err(ProofError::new(format!(
            "a {mode} composition has an empty separator"
        )))
    } else {
        Ok(())
    }
}

pub(super) fn remove_duplicate_h0(
    diagram: &mut Vec<ProofBar>,
    child_count: usize,
) -> Result<(), ProofError> {
    for _ in 1..child_count {
        let position = diagram
            .iter()
            .position(|bar| bar.dimension == 0 && bar.birth == 0.0 && bar.death == f64::INFINITY)
            .ok_or_else(|| ProofError::new("contractible composition lacks a child H0 class"))?;
        diagram.remove(position);
    }
    Ok(())
}

pub(super) fn has_zero_cone_center(graph: &Graph, separator: &[usize], threshold: f64) -> bool {
    separator.iter().any(|&center| {
        separator.iter().all(|&vertex| {
            let value = graph.get(center, vertex);
            center == vertex || (value.is_finite() && value == 0.0 && value <= threshold)
        })
    })
}

pub(super) fn validate_interface_scope(
    node: &InterfaceProof,
    graph: &Graph,
) -> Result<(), ProofError> {
    validate_vertex_scope(node, graph)?;
    validate_edge_scope(node, graph)?;
    validate_subscope(&node.separator, &node.vertices)?;
    validate_subscope(&node.protected_vertices, &node.vertices)
}

fn validate_vertex_scope(node: &InterfaceProof, graph: &Graph) -> Result<(), ProofError> {
    if node.vertices.is_empty()
        || node.vertices.windows(2).any(|pair| pair[0] >= pair[1])
        || node
            .vertices
            .iter()
            .any(|&vertex| vertex >= graph.vertex_count)
    {
        Err(ProofError::new("interface scope is not canonical"))
    } else {
        Ok(())
    }
}

fn validate_edge_scope(node: &InterfaceProof, graph: &Graph) -> Result<(), ProofError> {
    if node
        .edge_positions
        .windows(2)
        .any(|pair| pair[0] >= pair[1])
        || node
            .edge_positions
            .iter()
            .any(|&position| position >= graph.edges.len())
    {
        Err(ProofError::new("interface scope is not canonical"))
    } else {
        Ok(())
    }
}

fn validate_subscope(values: &[usize], vertices: &[usize]) -> Result<(), ProofError> {
    if values.windows(2).any(|pair| pair[0] >= pair[1])
        || values
            .iter()
            .any(|value| vertices.binary_search(value).is_err())
    {
        Err(ProofError::new("interface scope is not canonical"))
    } else {
        Ok(())
    }
}

pub(super) fn validate_relative_artifact(node: &InterfaceProof) -> Result<(), ProofError> {
    if (node.mode == InterfaceMode::Relative) == node.relative_artifact.is_empty() {
        Err(ProofError::new(
            "relative interface mode and artifact presence disagree",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn induced_edge_positions(graph: &Graph, vertices: &[usize]) -> Vec<usize> {
    graph
        .edges
        .iter()
        .enumerate()
        .filter_map(|(position, edge)| {
            (vertices.binary_search(&edge.u).is_ok() && vertices.binary_search(&edge.v).is_ok())
                .then_some(position)
        })
        .collect()
}

pub(super) fn resolve_children<'a>(
    node: &InterfaceProof,
    nodes: &'a BTreeMap<[u8; 32], InterfaceProof>,
) -> Result<Vec<&'a InterfaceProof>, ProofError> {
    if node.children.len() < 2 {
        return Err(ProofError::new(
            "an interface split has fewer than two children",
        ));
    }
    node.children
        .iter()
        .map(|digest| {
            nodes
                .get(digest)
                .ok_or_else(|| ProofError::new("interface references an unknown child"))
        })
        .collect()
}

pub(super) fn validate_child_protection(
    node: &InterfaceProof,
    children: &[&InterfaceProof],
) -> Result<(), ProofError> {
    for child in children {
        let expected =
            inherited_protection(&node.protected_vertices, &node.separator, &child.vertices);
        if child.protected_vertices != expected {
            return Err(ProofError::new(
                "interface child protects the wrong ancestor separators",
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_child_scopes(
    node: &InterfaceProof,
    children: &[&InterfaceProof],
) -> Result<(), ProofError> {
    let mut union = BTreeSet::new();
    for child in children {
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
        Err(ProofError::new(
            "interface children do not cover their parent vertices",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_child_intersections(
    node: &InterfaceProof,
    children: &[&InterfaceProof],
) -> Result<(), ProofError> {
    for left in 0..children.len() {
        for right in left + 1..children.len() {
            let intersection = children[left]
                .vertices
                .iter()
                .filter(|vertex| children[right].vertices.binary_search(vertex).is_ok())
                .copied()
                .collect::<Vec<_>>();
            if intersection != node.separator {
                return Err(ProofError::new(
                    "interface-child intersection differs from the declared separator",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn validate_child_edge_cover(
    node: &InterfaceProof,
    children: &[&InterfaceProof],
) -> Result<(), ProofError> {
    if node.edge_positions.iter().any(|position| {
        !children
            .iter()
            .any(|child| child.edge_positions.binary_search(position).is_ok())
    }) {
        Err(ProofError::new(
            "interface parent has an edge outside every child",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn verify_parent_relation(
    node: &InterfaceProof,
    parent: Option<&InterfaceProof>,
) -> Result<(), ProofError> {
    let Some(parent) = parent else {
        return if node.protected_vertices.is_empty() {
            Ok(())
        } else {
            Err(ProofError::new(
                "snapshot root has nonempty protected vertices",
            ))
        };
    };
    if node.vertices.len() >= parent.vertices.len() {
        return Err(ProofError::new(
            "snapshot child scope is not smaller than its parent",
        ));
    }
    let expected = inherited_protection(
        &parent.protected_vertices,
        &parent.separator,
        &node.vertices,
    );
    if node.protected_vertices != expected {
        Err(ProofError::new(
            "snapshot child protects the wrong ancestor separators",
        ))
    } else {
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
pub(super) struct CheckCounts {
    pub(super) nodes: usize,
    pub(super) composed_nodes: usize,
    pub(super) relative_nodes: usize,
    pub(super) edge_columns: usize,
    pub(super) triangle_columns: usize,
    pub(super) higher_columns: usize,
}

impl CheckCounts {
    pub(super) fn record(
        &mut self,
        node: &InterfaceProof,
        limits: ProofLimits,
    ) -> Result<(), ProofError> {
        self.nodes += 1;
        self.composed_nodes += usize::from(node.mode != InterfaceMode::Materialized);
        self.relative_nodes += usize::from(node.mode == InterfaceMode::Relative);
        self.add_node(node, limits)
    }

    pub(super) fn add_columns(&mut self, columns: &[Vec<ProofColumn>]) {
        self.edge_columns += columns.first().map_or(0, Vec::len);
        self.triangle_columns += columns.get(1).map_or(0, Vec::len);
        self.higher_columns += columns.iter().skip(2).map(Vec::len).sum::<usize>();
    }

    pub(super) fn add_node(
        &mut self,
        node: &InterfaceProof,
        limits: ProofLimits,
    ) -> Result<(), ProofError> {
        self.add_columns(&node.graded_columns);
        if node.mode == InterfaceMode::Relative {
            let relative = decode_verified(&node.relative_artifact, limits)?;
            self.add_columns(&relative.columns);
        }
        Ok(())
    }
}
pub(super) fn local_graph(graph: &Graph, node: &InterfaceProof) -> Result<Graph, ProofError> {
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
