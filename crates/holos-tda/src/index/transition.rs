use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{
    CertificateLimits, CorrespondenceMode, Diagram, DiagramDelta, EdgeKey, Error, ExplainedDiagram,
    Result, RipsParams, SparseDistanceMatrix, rips_persistence_with_classes_sparse,
};

use super::model::{
    IndexBranch, IndexDiff, IndexEdit, IndexEvent, IndexEventKind, IndexParams, IndexSummary,
    IndexTransition, IndexUpdateMode, IndexWork, InterfaceMode, InterfaceNode, InterfaceSummary,
    PersistenceIndex, TopologyPatch,
};
use super::summary::{
    collect_interfaces, correspondences, count_columns, diagram_delta, shared_nodes, summarize,
};

mod update;

use update::UpdateContext;

impl PersistenceIndex {
    /// Current diagram through the configured homology dimension.
    pub fn diagram(&self) -> &Diagram {
        self.root.diagram()
    }

    /// Current sparse graph state.
    pub fn graph(&self) -> &SparseDistanceMatrix {
        &self.graph
    }

    /// Structural and algebraic tree size.
    pub fn summary(&self) -> IndexSummary {
        self.summary
    }

    /// Content identifier of this index version.
    pub fn version(&self) -> [u8; 32] {
        self.root.digest
    }

    /// Persistence parameters shared by every version of this index.
    pub fn params(&self) -> &RipsParams {
        &self.params
    }

    pub(crate) fn root(&self) -> &Arc<InterfaceNode> {
        &self.root
    }

    pub(crate) fn topology(&self) -> &[EdgeKey] {
        &self.topology
    }

    pub(crate) fn certificate_limits(&self) -> CertificateLimits {
        self.limits
    }

    /// Separator search parameters used to compile this tree.
    pub fn index_params(&self) -> IndexParams {
        self.index_params
    }

    /// Every interface in deterministic preorder.
    pub fn interfaces(&self) -> Vec<InterfaceSummary> {
        let mut output = Vec::with_capacity(self.summary.nodes);
        collect_interfaces(&self.root, 0, &mut output);
        output
    }

    /// Compute canonical H1 class spaces for the current root on demand.
    ///
    /// The returned explanation covers H0 and H1 even when the index also
    /// maintains higher-dimensional bars.
    pub fn explain(&self) -> Result<ExplainedDiagram> {
        let mut params = self.params.clone();
        params.max_dim = 1;
        rips_persistence_with_classes_sparse(&self.graph, &params)
    }

    /// Number of tree nodes physically shared with another version.
    pub fn shared_nodes_with(&self, other: &Self) -> usize {
        shared_nodes(&self.root, &other.root)
    }

    /// Compare two versions without recomputing persistence.
    pub fn diff(&self, other: &Self) -> IndexDiff {
        IndexDiff {
            same_envelope: self.graph.len() == other.graph.len()
                && self.topology.as_ref() == other.topology.as_ref(),
            shared_nodes: self.shared_nodes_with(other),
            diagram: diagram_delta(self.diagram(), other.diagram()),
        }
    }

    /// Create another handle to the same version.
    pub fn fork(&self) -> Self {
        self.clone()
    }

    /// Apply local edits inside the listed-edge envelope.
    ///
    /// Deactivation requires a finite threshold. It stores `f64::MAX`.
    /// The edge stays listed and can be activated later.
    pub fn transition_edits(&self, edits: &[IndexEdit]) -> Result<IndexTransition> {
        self.transition_edits_with(edits, CorrespondenceMode::Omit)
    }

    /// Apply local edits with explicit class-relation control.
    pub fn transition_edits_with(
        &self,
        edits: &[IndexEdit],
        correspondence_mode: CorrespondenceMode,
    ) -> Result<IndexTransition> {
        if edits.is_empty() {
            return self.transition_fixed_envelope(
                &self.graph,
                BTreeSet::new(),
                correspondence_mode,
                0,
            );
        }
        let replacements = self.edit_replacements(edits)?;
        let triplets: Vec<_> = self
            .topology
            .iter()
            .map(|&edge| {
                (
                    edge.u,
                    edge.v,
                    replacements
                        .get(&edge)
                        .copied()
                        .unwrap_or_else(|| self.graph.get(edge.u, edge.v)),
                )
            })
            .collect();
        let updated = SparseDistanceMatrix::from_triplets(self.graph.len(), &triplets)?;
        let changed: BTreeSet<_> = replacements
            .iter()
            .filter(|(edge, value)| self.graph.get(edge.u, edge.v).to_bits() != value.to_bits())
            .map(|(edge, _)| {
                self.topology
                    .binary_search(edge)
                    .expect("the edge was checked")
            })
            .collect();
        self.transition_fixed_envelope(&updated, changed, correspondence_mode, edits.len())
    }

    fn edit_replacements(&self, edits: &[IndexEdit]) -> Result<BTreeMap<EdgeKey, f64>> {
        let mut replacements = BTreeMap::new();
        for &edit in edits {
            let edge = edit.edge();
            if self.topology.binary_search(&edge).is_err() {
                return Err(Error::InvalidInput(format!(
                    "index edit edge ({}, {}) is outside the listed-edge envelope",
                    edge.u, edge.v
                )));
            }
            let value = edit_value(edit, self.params.threshold)?;
            if replacements.insert(edge, value).is_some() {
                return Err(Error::InvalidInput(format!(
                    "index edit repeats edge ({}, {})",
                    edge.u, edge.v
                )));
            }
        }
        Ok(replacements)
    }

    /// Apply one atomic active-topology patch.
    pub fn transition_patch(&self, patch: &TopologyPatch) -> Result<IndexTransition> {
        self.transition_edits(patch.edits())
    }

    /// Apply one patch with explicit class-relation control.
    pub fn transition_patch_with(
        &self,
        patch: &TopologyPatch,
        correspondence_mode: CorrespondenceMode,
    ) -> Result<IndexTransition> {
        self.transition_edits_with(patch.edits(), correspondence_mode)
    }

    /// Advance an ordered batch atomically.
    ///
    /// If one update fails, the receiver stays unchanged.
    pub fn advance_batch(
        &mut self,
        updates: &[SparseDistanceMatrix],
    ) -> Result<Vec<IndexTransition>> {
        let mut candidate = self.clone();
        let mut transitions = Vec::with_capacity(updates.len());
        for update in updates {
            let transition = candidate.transition(update)?;
            candidate = transition.index.clone();
            transitions.push(transition);
        }
        *self = candidate;
        Ok(transitions)
    }

    /// Advance independent alternatives from this version.
    ///
    /// Output order matches input order. The receiver does not change. The
    /// configured persistence thread count bounds concurrent alternatives.
    pub fn branch(&self, alternatives: &[SparseDistanceMatrix]) -> Result<Vec<IndexBranch>> {
        if alternatives.is_empty() {
            return Ok(Vec::new());
        }
        if self.params.threads <= 1 || alternatives.len() == 1 {
            return alternatives
                .iter()
                .enumerate()
                .map(|(index, alternative)| {
                    Ok(IndexBranch {
                        index,
                        transition: self.transition(alternative)?,
                    })
                })
                .collect();
        }
        use rayon::prelude::*;
        let workers = self.params.threads.min(alternatives.len());
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .map_err(|error| {
                Error::InvalidInput(format!("cannot create index branch workers: {error}"))
            })?;
        let branches = pool.install(|| {
            alternatives
                .par_iter()
                .enumerate()
                .map(|(index, alternative)| {
                    Ok(IndexBranch {
                        index,
                        transition: self.transition(alternative)?,
                    })
                })
                .collect::<Vec<Result<IndexBranch>>>()
        });
        branches.into_iter().collect()
    }

    /// Create a new exact version and omit cross-state class relations.
    pub fn transition(&self, updated: &SparseDistanceMatrix) -> Result<IndexTransition> {
        self.transition_with(updated, CorrespondenceMode::Omit)
    }

    /// Create a new exact version with explicit class-relation control.
    pub fn transition_with(
        &self,
        updated: &SparseDistanceMatrix,
        correspondence_mode: CorrespondenceMode,
    ) -> Result<IndexTransition> {
        let old_diagram = self.diagram().clone();
        let updated_topology: Vec<_> = updated
            .edges()
            .map(|(u, v, _)| EdgeKey::new(u, v))
            .collect();
        if updated.len() != self.graph.len() || updated_topology != *self.topology {
            let index = Self::compile(updated, &self.params, self.index_params, self.limits)?;
            let correspondence = correspondences(self, &index, correspondence_mode)?;
            let work = IndexWork {
                edges_checked: self.topology.len().max(updated_topology.len()),
                nodes_touched: index.summary.nodes,
                nodes_rebuilt: index.summary.materialized_interfaces,
                nodes_composed: index.summary.composed_interfaces,
                relative_nodes_rebuilt: index.summary.relative_interfaces.min(index.summary.leaves),
                relative_nodes_composed: index
                    .summary
                    .relative_interfaces
                    .saturating_sub(index.summary.leaves),
                relative_input_cells: index.summary.relative_input_cells,
                relative_core_cells: index.summary.relative_core_cells,
                relative_cancellations: index.summary.relative_cancellations,
                reduction_columns_reduced: count_columns(&index.root),
                ..IndexWork::default()
            };
            return Ok(IndexTransition {
                delta: diagram_delta(&old_diagram, index.diagram()),
                index,
                mode: IndexUpdateMode::Recompiled,
                events: vec![IndexEvent {
                    kind: IndexEventKind::EnvelopeRecompiled,
                    node: None,
                    edge: None,
                }],
                correspondence,
                work,
            });
        }

        let changed: BTreeSet<_> = self
            .topology
            .iter()
            .enumerate()
            .filter_map(|(position, edge)| {
                (self.graph.get(edge.u, edge.v).to_bits() != updated.get(edge.u, edge.v).to_bits())
                    .then_some(position)
            })
            .collect();
        self.transition_fixed_envelope(updated, changed, correspondence_mode, self.topology.len())
    }

    fn transition_fixed_envelope(
        &self,
        updated: &SparseDistanceMatrix,
        changed: BTreeSet<usize>,
        correspondence_mode: CorrespondenceMode,
        edges_checked: usize,
    ) -> Result<IndexTransition> {
        if changed.is_empty() {
            return Ok(IndexTransition {
                index: self.clone(),
                mode: IndexUpdateMode::Unchanged,
                delta: DiagramDelta::default(),
                events: Vec::new(),
                correspondence: Vec::new(),
                work: IndexWork {
                    edges_checked,
                    nodes_shared: self.summary.nodes,
                    ..IndexWork::default()
                },
            });
        }

        let mut work = IndexWork {
            edges_checked,
            ..IndexWork::default()
        };
        let mut events = Vec::new();
        let threshold = self.params.threshold.unwrap_or(f64::INFINITY);
        let mut context = UpdateContext {
            current: &self.graph,
            updated,
            topology: &self.topology,
            changed: &changed,
            params: &self.params,
            interface_policy: self.index_params.interface_policy,
            limits: self.limits,
            threshold,
            work: &mut work,
            events: &mut events,
        };
        let root = context.update_node(&self.root)?;
        let index = Self {
            params: self.params.clone(),
            index_params: self.index_params,
            limits: self.limits,
            graph: Arc::new(updated.clone()),
            topology: Arc::clone(&self.topology),
            root,
            summary: IndexSummary::default(),
        };
        let mut index = index;
        index.summary.max_dim = self.params.max_dim;
        index.summary.separator_candidates_checked = self.summary.separator_candidates_checked;
        index.summary.separator_search_complete = self.summary.separator_search_complete;
        summarize(&index.root, &mut index.summary);
        index.summary.root_composed = index.root.mode() != InterfaceMode::Materialized;
        let correspondence = correspondences(self, &index, correspondence_mode)?;
        let mode = if work.nodes_rebuilt > 0 {
            IndexUpdateMode::Rebuilt
        } else if work.nodes_repaired > 0 {
            IndexUpdateMode::Repaired
        } else if work.relative_nodes_rebuilt > 0 || work.relative_nodes_composed > 0 {
            IndexUpdateMode::Relative
        } else {
            IndexUpdateMode::Composed
        };
        Ok(IndexTransition {
            delta: diagram_delta(self.diagram(), index.diagram()),
            index,
            mode,
            events,
            correspondence,
            work,
        })
    }

    /// Mutate this handle to the next version and return its transition.
    pub fn advance(&mut self, updated: &SparseDistanceMatrix) -> Result<IndexTransition> {
        let transition = self.transition(updated)?;
        *self = transition.index.clone();
        Ok(transition)
    }
}

fn edit_value(edit: IndexEdit, threshold: Option<f64>) -> Result<f64> {
    match edit {
        IndexEdit::SetWeight { value, .. } => Ok(value),
        IndexEdit::Activate { value, .. } => {
            let threshold = threshold.ok_or_else(|| {
                Error::InvalidInput("edge activation requires a finite index threshold".into())
            })?;
            if value > threshold {
                return Err(Error::InvalidInput(format!(
                    "edge activation value {value} exceeds the threshold {threshold}"
                )));
            }
            Ok(value)
        }
        IndexEdit::Deactivate { .. } => {
            let threshold = threshold.ok_or_else(|| {
                Error::InvalidInput("edge deactivation requires a finite index threshold".into())
            })?;
            if threshold >= f64::MAX {
                return Err(Error::InvalidInput(
                    "edge deactivation requires a threshold below f64::MAX".into(),
                ));
            }
            Ok(f64::MAX)
        }
    }
}
