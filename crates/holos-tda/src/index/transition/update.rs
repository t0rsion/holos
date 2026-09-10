use std::collections::BTreeSet;
use std::sync::Arc;

use crate::{
    CertificateLimits, EdgeKey, Error, GradedReductionCertificate, ReductionRepairMode,
    RelativeInterfaceCertificate, Result, RipsParams, SparseDistanceMatrix,
};

use super::super::compile::{Scope, compose_diagram, composition_mode, local_graph};
use super::super::model::{
    IndexEvent, IndexEventKind, IndexWork, InterfaceMode, InterfaceNode, InterfacePolicy,
    InterfaceState,
};
use super::super::summary::{count_nodes, node_digest};

pub(super) struct UpdateContext<'a> {
    pub(super) current: &'a SparseDistanceMatrix,
    pub(super) updated: &'a SparseDistanceMatrix,
    pub(super) topology: &'a [EdgeKey],
    pub(super) changed: &'a BTreeSet<usize>,
    pub(super) params: &'a RipsParams,
    pub(super) interface_policy: InterfacePolicy,
    pub(super) limits: CertificateLimits,
    pub(super) threshold: f64,
    pub(super) work: &'a mut IndexWork,
    pub(super) events: &'a mut Vec<IndexEvent>,
}

impl UpdateContext<'_> {
    pub(super) fn update_node(&mut self, node: &Arc<InterfaceNode>) -> Result<Arc<InterfaceNode>> {
        let Some(first_changed) = self.first_changed(node) else {
            self.work.nodes_shared += count_nodes(node);
            return Ok(Arc::clone(node));
        };
        self.work.nodes_touched += 1;
        let children = node
            .children
            .iter()
            .map(|child| self.update_node(child))
            .collect::<Result<Vec<_>>>()?;
        let crossing = self.crosses_threshold(node);
        let state = self.update_state(node, &children, first_changed, crossing)?;
        let digest = node_digest(
            &node.vertices,
            &node.edge_positions,
            &node.separator,
            &node.protected_vertices,
            &children,
            &state,
        );
        Ok(Arc::new(InterfaceNode {
            digest,
            vertices: node.vertices.clone(),
            edge_positions: node.edge_positions.clone(),
            separator: node.separator.clone(),
            protected_vertices: node.protected_vertices.clone(),
            children,
            state,
        }))
    }

    fn first_changed(&self, node: &InterfaceNode) -> Option<usize> {
        node.edge_positions
            .iter()
            .find(|position| self.changed.contains(position))
            .copied()
    }

    fn crosses_threshold(&self, node: &InterfaceNode) -> bool {
        node.edge_positions.iter().any(|&position| {
            let edge = self.topology[position];
            (self.current.get(edge.u, edge.v) <= self.threshold)
                != (self.updated.get(edge.u, edge.v) <= self.threshold)
        })
    }

    fn update_state(
        &mut self,
        node: &InterfaceNode,
        children: &[Arc<InterfaceNode>],
        first_changed: usize,
        crossing: bool,
    ) -> Result<InterfaceState> {
        if crossing {
            self.push_event(IndexEventKind::ThresholdCrossing, node, first_changed);
        }
        if self.interface_policy == InterfacePolicy::Relative {
            return self.relative_state(node, children, first_changed);
        }
        if let Some(mode) = self.composition(node, children) {
            return self.composed_state(node, children, first_changed, mode);
        }
        if crossing {
            return self.rebuild_state(node, first_changed);
        }
        self.repair_state(node, first_changed)
    }

    fn composition(
        &self,
        node: &InterfaceNode,
        children: &[Arc<InterfaceNode>],
    ) -> Option<InterfaceMode> {
        (self.interface_policy == InterfacePolicy::Compose)
            .then(|| composition_mode(&node.separator, children, self.updated, self.params))
            .flatten()
    }

    fn relative_state(
        &mut self,
        node: &InterfaceNode,
        children: &[Arc<InterfaceNode>],
        first_changed: usize,
    ) -> Result<InterfaceState> {
        let (relative, kind) = if children.is_empty() {
            let scope = node_scope(node);
            let local = local_graph(&scope, self.updated, self.topology)?;
            let relative = RelativeInterfaceCertificate::build_labeled(
                &local,
                &node.vertices,
                self.params,
                &node.protected_vertices,
                self.limits,
            )
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
            self.work.relative_nodes_rebuilt += 1;
            (relative, IndexEventKind::RelativeCoreRebuilt)
        } else {
            let relative =
                compose_relative_children(children, &node.protected_vertices, self.limits)?;
            self.work.relative_nodes_composed += 1;
            (relative, IndexEventKind::RelativeCoreComposed)
        };
        let relative_work = relative.work();
        self.work.relative_input_cells += relative_work.input_cells;
        self.work.relative_core_cells += relative_work.core_cells;
        self.work.relative_cancellations += relative_work.cancellations;
        self.push_event(kind, node, first_changed);
        Ok(InterfaceState::Relative(relative))
    }

    fn composed_state(
        &mut self,
        node: &InterfaceNode,
        children: &[Arc<InterfaceNode>],
        first_changed: usize,
        mode: InterfaceMode,
    ) -> Result<InterfaceState> {
        self.work.nodes_composed += 1;
        self.push_event(IndexEventKind::InterfaceComposed, node, first_changed);
        Ok(InterfaceState::Composed {
            mode,
            diagram: compose_diagram(children, mode)?,
        })
    }

    fn rebuild_state(
        &mut self,
        node: &InterfaceNode,
        first_changed: usize,
    ) -> Result<InterfaceState> {
        let reduction = self.build_reduction(node)?;
        self.work.nodes_rebuilt += 1;
        self.work.reduction_columns_reduced += reduction.column_count();
        self.push_event(IndexEventKind::ReductionRebuilt, node, first_changed);
        Ok(InterfaceState::Materialized(reduction))
    }

    fn repair_state(
        &mut self,
        node: &InterfaceNode,
        first_changed: usize,
    ) -> Result<InterfaceState> {
        let Some(reduction) = node.reduction() else {
            return self.rebuild_state(node, first_changed);
        };
        let scope = node_scope(node);
        let old_local = local_graph(&scope, self.current, self.topology)?;
        let new_local = local_graph(&scope, self.updated, self.topology)?;
        let repair = reduction
            .repair(&old_local, &new_local, self.limits)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        let mode = repair.mode();
        let repair_work = repair.work();
        self.work.reduction_columns_reused += repair_work.columns_reused();
        self.work.reduction_columns_reduced += repair_work.columns_reduced();
        self.work.reduction_column_additions += repair_work.column_additions();
        let kind = match mode {
            ReductionRepairMode::Reused | ReductionRepairMode::SuffixRepaired => {
                self.work.nodes_repaired += 1;
                IndexEventKind::ReductionRepaired
            }
            ReductionRepairMode::Rebuilt => {
                self.work.nodes_rebuilt += 1;
                IndexEventKind::ReductionRebuilt
            }
        };
        self.push_event(kind, node, first_changed);
        Ok(InterfaceState::Materialized(repair.into_certificate()))
    }

    fn build_reduction(&self, node: &InterfaceNode) -> Result<GradedReductionCertificate> {
        let scope = node_scope(node);
        let local = local_graph(&scope, self.updated, self.topology)?;
        GradedReductionCertificate::build(&local, self.params, self.limits)
            .map_err(|error| Error::InvalidInput(error.to_string()))
    }

    fn push_event(&mut self, kind: IndexEventKind, node: &InterfaceNode, edge_position: usize) {
        self.events.push(IndexEvent {
            kind,
            node: Some(node.digest),
            edge: Some(self.topology[edge_position]),
        });
    }
}

fn node_scope(node: &InterfaceNode) -> Scope {
    Scope {
        vertices: node.vertices.clone(),
        edge_positions: node.edge_positions.clone(),
    }
}

fn compose_relative_children(
    children: &[Arc<InterfaceNode>],
    protected_vertices: &[usize],
    limits: CertificateLimits,
) -> Result<RelativeInterfaceCertificate> {
    let relative_children = children
        .iter()
        .map(|child| {
            child.relative().ok_or_else(|| {
                Error::InvalidInput("a relative index parent requires relative child cores".into())
            })
        })
        .collect::<Result<Vec<_>>>()?;
    RelativeInterfaceCertificate::compose_trusted(&relative_children, protected_vertices, limits)
        .map_err(|error| Error::InvalidInput(error.to_string()))
}
