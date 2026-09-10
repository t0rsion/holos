use std::collections::BTreeMap;
use std::sync::Arc;

use crate::{
    CertificateLimits, Diagram, EdgeKey, Error, GradedReductionCertificate,
    RelativeInterfaceCertificate, Result, RipsParams, SparseDistanceMatrix,
};

use super::super::model::{InterfaceMode, InterfaceNode, InterfacePolicy, InterfaceState};
use super::super::summary::node_digest;
use super::Scope;
use super::decompose::TreeSpec;

#[allow(clippy::too_many_arguments)]
pub(super) fn compile_node(
    spec: &TreeSpec,
    graph: &SparseDistanceMatrix,
    topology: &[EdgeKey],
    params: &RipsParams,
    interface_policy: InterfacePolicy,
    limits: CertificateLimits,
    protected_vertices: &[usize],
) -> Result<Arc<InterfaceNode>> {
    let children = compile_children(
        spec,
        graph,
        topology,
        params,
        interface_policy,
        limits,
        protected_vertices,
    )?;
    let state = compile_state(
        spec,
        &children,
        graph,
        topology,
        params,
        interface_policy,
        limits,
        protected_vertices,
    )?;
    let digest = node_digest(
        &spec.scope.vertices,
        &spec.scope.edge_positions,
        &spec.separator,
        protected_vertices,
        &children,
        &state,
    );
    Ok(Arc::new(InterfaceNode {
        digest,
        vertices: spec.scope.vertices.clone(),
        edge_positions: spec.scope.edge_positions.clone(),
        separator: spec.separator.clone(),
        protected_vertices: protected_vertices.to_vec(),
        children,
        state,
    }))
}

#[allow(clippy::too_many_arguments)]
fn compile_children(
    spec: &TreeSpec,
    graph: &SparseDistanceMatrix,
    topology: &[EdgeKey],
    params: &RipsParams,
    interface_policy: InterfacePolicy,
    limits: CertificateLimits,
    protected_vertices: &[usize],
) -> Result<Vec<Arc<InterfaceNode>>> {
    spec.children
        .iter()
        .map(|child| {
            let child_protected =
                inherited_protection(protected_vertices, &spec.separator, &child.scope.vertices);
            compile_node(
                child,
                graph,
                topology,
                params,
                interface_policy,
                limits,
                &child_protected,
            )
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn compile_state(
    spec: &TreeSpec,
    children: &[Arc<InterfaceNode>],
    graph: &SparseDistanceMatrix,
    topology: &[EdgeKey],
    params: &RipsParams,
    interface_policy: InterfacePolicy,
    limits: CertificateLimits,
    protected_vertices: &[usize],
) -> Result<InterfaceState> {
    match interface_policy {
        InterfacePolicy::Relative => compile_relative_state(
            spec,
            children,
            graph,
            topology,
            params,
            limits,
            protected_vertices,
        ),
        InterfacePolicy::Compose => {
            if let Some(mode) = composition_mode(&spec.separator, children, graph, params) {
                Ok(InterfaceState::Composed {
                    mode,
                    diagram: compose_diagram(children, mode)?,
                })
            } else {
                compile_materialized_state(&spec.scope, graph, topology, params, limits)
            }
        }
        InterfacePolicy::Materialize => {
            compile_materialized_state(&spec.scope, graph, topology, params, limits)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compile_relative_state(
    spec: &TreeSpec,
    children: &[Arc<InterfaceNode>],
    graph: &SparseDistanceMatrix,
    topology: &[EdgeKey],
    params: &RipsParams,
    limits: CertificateLimits,
    protected_vertices: &[usize],
) -> Result<InterfaceState> {
    let relative = if children.is_empty() {
        let local = local_graph(&spec.scope, graph, topology)?;
        RelativeInterfaceCertificate::build_labeled(
            &local,
            &spec.scope.vertices,
            params,
            protected_vertices,
            limits,
        )
    } else {
        let relative_children = children
            .iter()
            .map(|child| {
                child.relative().ok_or_else(|| {
                    crate::CertificateError::new(
                        "a relative index parent requires relative child cores",
                    )
                })
            })
            .collect::<std::result::Result<Vec<_>, _>>()?;
        RelativeInterfaceCertificate::compose_trusted(
            &relative_children,
            protected_vertices,
            limits,
        )
    }
    .map_err(|error| Error::InvalidInput(error.to_string()))?;
    Ok(InterfaceState::Relative(relative))
}

fn compile_materialized_state(
    scope: &Scope,
    graph: &SparseDistanceMatrix,
    topology: &[EdgeKey],
    params: &RipsParams,
    limits: CertificateLimits,
) -> Result<InterfaceState> {
    let local = local_graph(scope, graph, topology)?;
    let reduction = GradedReductionCertificate::build(&local, params, limits)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    Ok(InterfaceState::Materialized(reduction))
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

pub(crate) fn composition_mode(
    separator: &[usize],
    children: &[Arc<InterfaceNode>],
    graph: &SparseDistanceMatrix,
    params: &RipsParams,
) -> Option<InterfaceMode> {
    if children.is_empty() {
        return None;
    }
    if separator.is_empty() {
        return Some(InterfaceMode::Disjoint);
    }
    let threshold = params.threshold.unwrap_or(f64::INFINITY);
    let zero_simplex = separator.iter().enumerate().all(|(index, &u)| {
        separator[index + 1..]
            .iter()
            .all(|&v| graph.get(u, v) == 0.0 && graph.get(u, v) <= threshold)
    });
    if zero_simplex {
        return Some(InterfaceMode::ZeroSimplex);
    }
    let all_active_edges_are_zero = separator.iter().enumerate().all(|(index, &u)| {
        separator[index + 1..].iter().all(|&v| {
            let value = graph.get(u, v);
            !value.is_finite() || value > threshold || value == 0.0
        })
    });
    let has_zero_cone_vertex = separator.iter().any(|&center| {
        separator.iter().all(|&vertex| {
            center == vertex
                || (graph.get(center, vertex).is_finite()
                    && graph.get(center, vertex) == 0.0
                    && graph.get(center, vertex) <= threshold)
        })
    });
    (all_active_edges_are_zero && has_zero_cone_vertex).then_some(InterfaceMode::ZeroCone)
}

pub(crate) fn compose_diagram(
    children: &[Arc<InterfaceNode>],
    mode: InterfaceMode,
) -> Result<Diagram> {
    let mut bars: Vec<_> = children
        .iter()
        .flat_map(|child| child.diagram().bars.iter().cloned())
        .collect();
    if matches!(mode, InterfaceMode::ZeroSimplex | InterfaceMode::ZeroCone) {
        for _ in 1..children.len() {
            let position = bars
                .iter()
                .position(|bar| bar.dim == 0 && bar.birth == 0.0 && bar.is_essential())
                .ok_or_else(|| {
                    Error::InvalidInput(
                        "a contractible composition requires one essential H0 class per child"
                            .into(),
                    )
                })?;
            bars.remove(position);
        }
    }
    let mut diagram = Diagram { bars };
    diagram.canonicalize();
    Ok(diagram)
}

pub(crate) fn local_graph(
    scope: &Scope,
    graph: &SparseDistanceMatrix,
    topology: &[EdgeKey],
) -> Result<SparseDistanceMatrix> {
    let positions: BTreeMap<_, _> = scope
        .vertices
        .iter()
        .copied()
        .enumerate()
        .map(|(local, original)| (original, local))
        .collect();
    let triplets: Vec<_> = scope
        .edge_positions
        .iter()
        .map(|&position| {
            let edge = topology[position];
            (
                positions[&edge.u],
                positions[&edge.v],
                graph.get(edge.u, edge.v),
            )
        })
        .collect();
    SparseDistanceMatrix::from_triplets(scope.vertices.len(), &triplets)
}
