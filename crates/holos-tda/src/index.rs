//! Versioned exact persistence over checked separator interfaces.
//!
//! An index decomposes the fixed listed-edge envelope of a sparse graph.
//! Relative filtered cores compose through arbitrary protected separators.
//! `InterfacePolicy::Compose` and `InterfacePolicy::Materialize` are
//! alternative parent policies. A transition path-copies the affected route
//! and shares every untouched subtree.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::{
    Bar, CertificateLimits, ClassCorrespondence, CorrespondenceMode, Diagram, EdgeKey, Error,
    ExplainedDiagram, GradedReductionCertificate, ReductionRepairMode,
    RelativeInterfaceCertificate, Result, RipsParams, SparseDistanceMatrix, class_correspondences,
    rips_persistence_with_classes_sparse,
};

/// Bounds and decomposition choices for a [`PersistenceIndex`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct IndexParams {
    /// Largest vertex separator considered by the deterministic search.
    pub max_separator_width: usize,
    /// Largest total candidate count across one tree compilation.
    pub separator_search_limit: usize,
    /// A scope at or below this vertex count remains a leaf.
    pub leaf_vertices: usize,
    /// How parent interfaces compose or retain reductions.
    pub interface_policy: InterfacePolicy,
}

impl Default for IndexParams {
    fn default() -> Self {
        Self {
            max_separator_width: 4,
            separator_search_limit: 100_000,
            leaf_vertices: 4,
            interface_policy: InterfacePolicy::Relative,
        }
    }
}

/// Policy for exact parent interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfacePolicy {
    /// Compose exact relative cores through arbitrary protected separators.
    Relative,
    /// Compose at a certified separator and materialize other parents.
    Compose,
    /// Retain a reduction over every parent scope.
    Materialize,
}

/// Structural and algebraic size of one compiled separator tree.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexSummary {
    /// Highest homology dimension maintained by this index.
    pub max_dim: usize,
    /// Interface nodes in the tree, including the root.
    pub nodes: usize,
    /// Nodes without children.
    pub leaves: usize,
    /// Nodes split by a nonempty separator.
    pub separators: usize,
    /// Nodes split into disconnected components.
    pub component_splits: usize,
    /// Largest separator used by the tree.
    pub widest_separator: usize,
    /// Largest vertex scope retained by one materialized interface.
    pub largest_interface_vertices: usize,
    /// Largest listed-edge scope retained by one materialized interface.
    pub largest_interface_edges: usize,
    /// Interfaces composed without a reduction over their full scope.
    pub composed_interfaces: usize,
    /// Interfaces that retain a checked reduction over their full scope.
    pub materialized_interfaces: usize,
    /// Interfaces that retain an exact relative filtered core.
    pub relative_interfaces: usize,
    /// Cells supplied to all relative interfaces before cancellation.
    pub relative_input_cells: usize,
    /// Cells retained by all relative interfaces after cancellation.
    pub relative_core_cells: usize,
    /// Largest cell count retained by one relative interface.
    pub largest_relative_core_cells: usize,
    /// Equal-filtration pairs removed across all relative interfaces.
    pub relative_cancellations: usize,
    /// Whether the root omits a reduction over its full scope.
    pub root_composed: bool,
    /// Candidate vertex sets checked during decomposition.
    pub separator_candidates_checked: usize,
    /// Whether the bounded search visited every candidate in scope.
    pub separator_search_complete: bool,
}

/// Exact work charged to one index transition.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexWork {
    /// Envelope edges compared when a full graph was supplied.
    pub edges_checked: usize,
    /// Tree nodes whose scopes contained at least one changed edge.
    pub nodes_touched: usize,
    /// Tree nodes shared with the preceding version.
    pub nodes_shared: usize,
    /// Touched reductions that retained at least one dependency column.
    pub nodes_repaired: usize,
    /// Touched reductions rebuilt without a retained dependency column.
    pub nodes_rebuilt: usize,
    /// Touched interfaces composed from their direct children.
    pub nodes_composed: usize,
    /// Touched relative leaf cores rebuilt from their local graph.
    pub relative_nodes_rebuilt: usize,
    /// Touched relative parent cores recomposed from child cores.
    pub relative_nodes_composed: usize,
    /// Cells supplied to touched relative interfaces before cancellation.
    pub relative_input_cells: usize,
    /// Cells retained by touched relative interfaces after cancellation.
    pub relative_core_cells: usize,
    /// Equal-filtration pairs removed in touched relative interfaces.
    pub relative_cancellations: usize,
    /// Reduction columns retained without another reduction pass.
    pub reduction_columns_reused: usize,
    /// Reduction columns processed by repair or rebuild.
    pub reduction_columns_reduced: usize,
    /// Sparse column additions performed during repair.
    pub reduction_column_additions: usize,
}

/// How an index produced a new version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexUpdateMode {
    /// The supplied graph was bit-for-bit equal to the current graph.
    Unchanged,
    /// Every changed node retained at least one reduction column.
    Repaired,
    /// Changed routes required no reduction over a parent scope.
    Composed,
    /// Changed routes were rebuilt from exact local relative cores.
    Relative,
    /// At least one changed node rebuilt its complete reduction.
    Rebuilt,
    /// The listed-edge envelope changed and a new tree was compiled.
    Recompiled,
}

/// Why an index transition changed execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IndexEventKind {
    /// An edge crossed the fixed filtration threshold.
    ThresholdCrossing,
    /// A reduction retained a valid dependency prefix.
    ReductionRepaired,
    /// A reduction retained no dependency prefix.
    ReductionRebuilt,
    /// A parent diagram was composed from checked child interfaces.
    InterfaceComposed,
    /// A relative leaf core was rebuilt from its local graph.
    RelativeCoreRebuilt,
    /// A relative parent core was recomposed from direct child cores.
    RelativeCoreComposed,
    /// The fixed listed-edge envelope changed.
    EnvelopeRecompiled,
}

/// One event reported by an index transition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEvent {
    /// Event kind.
    pub kind: IndexEventKind,
    /// Content identifier of the affected old node, when one exists.
    pub node: Option<[u8; 32]>,
    /// First changed edge in the node, when one exists.
    pub edge: Option<EdgeKey>,
}

/// Exact multiset difference between two persistence diagrams.
#[derive(Debug, Clone, Default)]
pub struct DiagramDelta {
    /// Intervals removed from the preceding version.
    pub removed: Vec<Bar>,
    /// Intervals added in the new version.
    pub added: Vec<Bar>,
}

/// One change to an edge already present in an index envelope.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum IndexEdit {
    /// Set the edge to the supplied non-negative finite weight.
    SetWeight {
        /// Edge in the fixed listed-edge envelope.
        edge: EdgeKey,
        /// New edge weight.
        value: f64,
    },
    /// Move an envelope edge into the finite filtration.
    Activate {
        /// Edge in the fixed listed-edge envelope.
        edge: EdgeKey,
        /// New edge weight at or below the finite threshold.
        value: f64,
    },
    /// Keep the edge listed but place it above the finite threshold.
    Deactivate {
        /// Edge in the fixed listed-edge envelope.
        edge: EdgeKey,
    },
}

impl IndexEdit {
    /// Construct a weight change in canonical endpoint order.
    pub fn set_weight(u: usize, v: usize, value: f64) -> Self {
        Self::SetWeight {
            edge: EdgeKey::new(u, v),
            value,
        }
    }

    /// Construct an edge deactivation in canonical endpoint order.
    pub fn deactivate(u: usize, v: usize) -> Self {
        Self::Deactivate {
            edge: EdgeKey::new(u, v),
        }
    }

    /// Construct an edge activation in canonical endpoint order.
    pub fn activate(u: usize, v: usize, value: f64) -> Self {
        Self::Activate {
            edge: EdgeKey::new(u, v),
            value,
        }
    }

    fn edge(self) -> EdgeKey {
        match self {
            Self::SetWeight { edge, .. }
            | Self::Activate { edge, .. }
            | Self::Deactivate { edge } => edge,
        }
    }
}

/// One atomic active-topology patch inside a fixed edge envelope.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TopologyPatch {
    edits: Vec<IndexEdit>,
}

impl TopologyPatch {
    /// Construct a patch from edge edits applied as one transaction.
    pub fn new(edits: Vec<IndexEdit>) -> Self {
        Self { edits }
    }

    /// Edits in this transaction.
    pub fn edits(&self) -> &[IndexEdit] {
        &self.edits
    }

    /// Consume this patch and return its edits.
    pub fn into_edits(self) -> Vec<IndexEdit> {
        self.edits
    }
}

/// Difference between two index versions.
#[derive(Debug, Clone)]
pub struct IndexDiff {
    /// Whether both versions use the same listed-edge envelope.
    pub same_envelope: bool,
    /// Tree nodes physically shared by both versions.
    pub shared_nodes: usize,
    /// Exact persistence-diagram difference.
    pub diagram: DiagramDelta,
}

/// One ordered alternative advanced from a shared index version.
#[derive(Debug, Clone)]
pub struct IndexBranch {
    /// Position of the alternative in the input batch.
    pub index: usize,
    /// Exact transition from the shared branch point.
    pub transition: IndexTransition,
}

/// Summary of one filtered interface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceSummary {
    /// Content identifier of this node.
    pub digest: [u8; 32],
    /// Depth from the root.
    pub depth: usize,
    /// Original labeled vertices in the node scope.
    pub vertices: Vec<usize>,
    /// Original labeled separator vertices shared by its children.
    pub separator: Vec<usize>,
    /// Original labeled vertices fixed for every ancestor composition.
    pub protected_vertices: Vec<usize>,
    /// Listed edges in the node scope.
    pub edges: usize,
    /// Direct child count.
    pub children: usize,
    /// Algebra used by this interface.
    pub mode: InterfaceMode,
    /// Boundary columns in the checked reduction.
    pub reduction_columns: usize,
    /// Boundary-column counts for simplex dimensions one through `max_dim + 1`.
    pub columns_by_dimension: Vec<usize>,
    /// Cells supplied before relative cancellation.
    pub relative_input_cells: usize,
    /// Cells retained by the relative interface.
    pub relative_core_cells: usize,
    /// Equal-filtration pairs removed by relative cancellation.
    pub relative_cancellations: usize,
}

/// Algebra retained by one separator interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfaceMode {
    /// The interface stores a filtered core relative to its parent boundary.
    Relative,
    /// The interface stores a reduction over its full vertex scope.
    Materialized,
    /// The scope is a disjoint union of its child scopes.
    Disjoint,
    /// Child scopes meet in one zero-filtration simplex.
    ZeroSimplex,
    /// Child scopes meet in a zero-filtration flag cone.
    ZeroCone,
}

/// Result of constructing a new index version.
#[derive(Debug, Clone)]
pub struct IndexTransition {
    /// New exact index version.
    pub index: PersistenceIndex,
    /// How the version was produced.
    pub mode: IndexUpdateMode,
    /// Exact diagram difference.
    pub delta: DiagramDelta,
    /// Transition events.
    pub events: Vec<IndexEvent>,
    /// Optional exact class relations on the common filtered subcomplex.
    pub correspondence: Vec<ClassCorrespondence>,
    /// Exact work charged to this transition.
    pub work: IndexWork,
}

#[derive(Debug, Clone)]
pub(crate) struct InterfaceNode {
    pub(crate) digest: [u8; 32],
    pub(crate) vertices: Vec<usize>,
    pub(crate) edge_positions: Vec<usize>,
    pub(crate) separator: Vec<usize>,
    pub(crate) protected_vertices: Vec<usize>,
    pub(crate) children: Vec<Arc<InterfaceNode>>,
    pub(crate) state: InterfaceState,
}

#[derive(Debug, Clone)]
pub(crate) enum InterfaceState {
    Relative(RelativeInterfaceCertificate),
    Materialized(GradedReductionCertificate),
    Composed {
        mode: InterfaceMode,
        diagram: Diagram,
    },
}

impl InterfaceNode {
    pub(crate) fn diagram(&self) -> &Diagram {
        match &self.state {
            InterfaceState::Relative(relative) => relative.diagram(),
            InterfaceState::Materialized(reduction) => reduction.diagram(),
            InterfaceState::Composed { diagram, .. } => diagram,
        }
    }

    pub(crate) fn mode(&self) -> InterfaceMode {
        match &self.state {
            InterfaceState::Relative(_) => InterfaceMode::Relative,
            InterfaceState::Materialized(_) => InterfaceMode::Materialized,
            InterfaceState::Composed { mode, .. } => *mode,
        }
    }

    pub(crate) fn reduction(&self) -> Option<&GradedReductionCertificate> {
        match &self.state {
            InterfaceState::Materialized(reduction) => Some(reduction),
            InterfaceState::Relative(_) | InterfaceState::Composed { .. } => None,
        }
    }

    pub(crate) fn relative(&self) -> Option<&RelativeInterfaceCertificate> {
        match &self.state {
            InterfaceState::Relative(relative) => Some(relative),
            InterfaceState::Materialized(_) | InterfaceState::Composed { .. } => None,
        }
    }
}

/// An immutable, versioned exact persistence index.
///
/// The listed vertices and edges form an envelope. Weight changes and
/// threshold crossings inside that envelope preserve the separator tree.
/// A different listed-edge set compiles a new tree and reports that event.
#[derive(Debug, Clone)]
pub struct PersistenceIndex {
    params: RipsParams,
    index_params: IndexParams,
    limits: CertificateLimits,
    graph: Arc<SparseDistanceMatrix>,
    topology: Arc<Vec<EdgeKey>>,
    root: Arc<InterfaceNode>,
    summary: IndexSummary,
}

impl PersistenceIndex {
    /// Compile an exact separator-tree index through `params.max_dim`.
    pub fn compile(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        index_params: IndexParams,
        limits: CertificateLimits,
    ) -> Result<Self> {
        validate_params(params, index_params)?;
        let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        let scope = Scope {
            vertices: (0..input.len()).collect(),
            edge_positions: (0..topology.len()).collect(),
        };
        let mut search = SeparatorSearch::new(&topology, index_params);
        let tree = search.decompose(scope);
        let root = compile_node(
            &tree,
            input,
            &topology,
            params,
            index_params.interface_policy,
            limits,
            &[],
        )?;
        let mut summary = IndexSummary {
            max_dim: params.max_dim,
            separator_candidates_checked: search.checked,
            separator_search_complete: search.complete,
            ..IndexSummary::default()
        };
        summarize(&root, &mut summary);
        summary.root_composed = root.mode() != InterfaceMode::Materialized;
        Ok(Self {
            params: params.clone(),
            index_params,
            limits,
            graph: Arc::new(input.clone()),
            topology: Arc::new(topology),
            root,
            summary,
        })
    }

    /// Exact current diagram through the configured homology dimension.
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

#[derive(Clone)]
struct Scope {
    vertices: Vec<usize>,
    edge_positions: Vec<usize>,
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

struct TreeSpec {
    scope: Scope,
    separator: Vec<usize>,
    children: Vec<TreeSpec>,
}

struct SeparatorSearch<'a> {
    topology: &'a [EdgeKey],
    params: IndexParams,
    checked: usize,
    complete: bool,
}

impl<'a> SeparatorSearch<'a> {
    fn new(topology: &'a [EdgeKey], params: IndexParams) -> Self {
        Self {
            topology,
            params,
            checked: 0,
            complete: true,
        }
    }

    fn decompose(&mut self, scope: Scope) -> TreeSpec {
        if scope.vertices.len() <= self.params.leaf_vertices || !self.complete {
            return TreeSpec {
                scope,
                separator: Vec::new(),
                children: Vec::new(),
            };
        }
        let components = scope_components(&scope, &[], self.topology);
        let choice = if components.len() > 1 {
            Some((Vec::new(), components))
        } else {
            self.find_separator(&scope)
        };
        let Some((separator, components)) = choice else {
            return TreeSpec {
                scope,
                separator: Vec::new(),
                children: Vec::new(),
            };
        };
        let children = components
            .into_iter()
            .map(|component| {
                let mut vertices = separator.clone();
                vertices.extend(component);
                vertices.sort_unstable();
                vertices.dedup();
                let members: BTreeSet<_> = vertices.iter().copied().collect();
                let edge_positions = scope
                    .edge_positions
                    .iter()
                    .copied()
                    .filter(|&position| {
                        let edge = self.topology[position];
                        members.contains(&edge.u) && members.contains(&edge.v)
                    })
                    .collect();
                self.decompose(Scope {
                    vertices,
                    edge_positions,
                })
            })
            .collect();
        TreeSpec {
            scope,
            separator,
            children,
        }
    }

    fn find_separator(&mut self, scope: &Scope) -> Option<(Vec<usize>, Vec<Vec<usize>>)> {
        let maximum = self
            .params
            .max_separator_width
            .min(scope.vertices.len().saturating_sub(2));
        for width in 1..=maximum {
            let mut positions: Vec<_> = (0..width).collect();
            let mut best: Option<(usize, Vec<usize>, Vec<Vec<usize>>)> = None;
            loop {
                if self.checked == self.params.separator_search_limit {
                    self.complete = false;
                    break;
                }
                self.checked += 1;
                let separator: Vec<_> = positions
                    .iter()
                    .map(|&position| scope.vertices[position])
                    .collect();
                let components = scope_components(scope, &separator, self.topology);
                if components.len() > 1 {
                    let largest = components.iter().map(Vec::len).max().unwrap_or(0);
                    let replace = best
                        .as_ref()
                        .map(|(old, old_separator, _)| {
                            (largest, &separator) < (*old, old_separator)
                        })
                        .unwrap_or(true);
                    if replace {
                        best = Some((largest, separator, components));
                    }
                }
                if !next_combination(&mut positions, scope.vertices.len()) {
                    break;
                }
            }
            if let Some((_, separator, components)) = best {
                return Some((separator, components));
            }
            if !self.complete {
                break;
            }
        }
        None
    }
}

fn validate_params(_params: &RipsParams, index: IndexParams) -> Result<()> {
    if index.leaf_vertices < 2 {
        return Err(Error::InvalidInput(
            "index leaf_vertices must be at least 2".into(),
        ));
    }
    if index.separator_search_limit == 0 {
        return Err(Error::InvalidInput(
            "index separator_search_limit must be positive".into(),
        ));
    }
    Ok(())
}

fn scope_components(scope: &Scope, separator: &[usize], topology: &[EdgeKey]) -> Vec<Vec<usize>> {
    let excluded: BTreeSet<_> = separator.iter().copied().collect();
    let members: BTreeSet<_> = scope.vertices.iter().copied().collect();
    let adjacency = scope_adjacency(scope, topology, &members, &excluded);
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    for &root in adjacency.keys() {
        if seen.insert(root) {
            components.push(walk_component(root, &adjacency, &mut seen));
        }
    }
    components.sort_unstable();
    components
}

fn scope_adjacency(
    scope: &Scope,
    topology: &[EdgeKey],
    members: &BTreeSet<usize>,
    excluded: &BTreeSet<usize>,
) -> BTreeMap<usize, Vec<usize>> {
    let mut adjacency = BTreeMap::<usize, Vec<usize>>::new();
    for &vertex in &scope.vertices {
        if !excluded.contains(&vertex) {
            adjacency.insert(vertex, Vec::new());
        }
    }
    for &position in &scope.edge_positions {
        let edge = topology[position];
        if members.contains(&edge.u)
            && members.contains(&edge.v)
            && !excluded.contains(&edge.u)
            && !excluded.contains(&edge.v)
        {
            adjacency.get_mut(&edge.u).unwrap().push(edge.v);
            adjacency.get_mut(&edge.v).unwrap().push(edge.u);
        }
    }
    adjacency
}

fn walk_component(
    root: usize,
    adjacency: &BTreeMap<usize, Vec<usize>>,
    seen: &mut BTreeSet<usize>,
) -> Vec<usize> {
    let mut stack = vec![root];
    let mut component = Vec::new();
    while let Some(vertex) = stack.pop() {
        component.push(vertex);
        for &neighbor in &adjacency[&vertex] {
            if seen.insert(neighbor) {
                stack.push(neighbor);
            }
        }
    }
    component.sort_unstable();
    component
}

fn next_combination(positions: &mut [usize], universe: usize) -> bool {
    for index in (0..positions.len()).rev() {
        let maximum = universe - (positions.len() - index);
        if positions[index] < maximum {
            positions[index] += 1;
            for next in index + 1..positions.len() {
                positions[next] = positions[next - 1] + 1;
            }
            return true;
        }
    }
    false
}

fn compile_node(
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

fn composition_mode(
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

fn compose_diagram(children: &[Arc<InterfaceNode>], mode: InterfaceMode) -> Result<Diagram> {
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

struct UpdateContext<'a> {
    current: &'a SparseDistanceMatrix,
    updated: &'a SparseDistanceMatrix,
    topology: &'a [EdgeKey],
    changed: &'a BTreeSet<usize>,
    params: &'a RipsParams,
    interface_policy: InterfacePolicy,
    limits: CertificateLimits,
    threshold: f64,
    work: &'a mut IndexWork,
    events: &'a mut Vec<IndexEvent>,
}

impl UpdateContext<'_> {
    fn update_node(&mut self, node: &Arc<InterfaceNode>) -> Result<Arc<InterfaceNode>> {
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

fn local_graph(
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

fn node_digest(
    vertices: &[usize],
    edge_positions: &[usize],
    separator: &[usize],
    protected_vertices: &[usize],
    children: &[Arc<InterfaceNode>],
    state: &InterfaceState,
) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"holos-filtered-interface-v4");
    digest_usizes(&mut hash, vertices);
    digest_usizes(&mut hash, edge_positions);
    digest_usizes(&mut hash, separator);
    digest_usizes(&mut hash, protected_vertices);
    hash.update((children.len() as u64).to_be_bytes());
    for child in children {
        hash.update(child.digest);
    }
    let diagram = digest_interface_state(&mut hash, state);
    digest_diagram(&mut hash, diagram);
    hash.finalize().into()
}

fn digest_interface_state<'a>(hash: &mut Sha256, state: &'a InterfaceState) -> &'a Diagram {
    match state {
        InterfaceState::Relative(relative) => {
            hash.update([4]);
            hash.update(relative.source_digest());
            hash.update(relative.digest());
            relative.diagram()
        }
        InterfaceState::Materialized(reduction) => {
            hash.update([0]);
            hash.update(reduction.graph_digest());
            hash.update((reduction.max_dim() as u64).to_be_bytes());
            hash.update((reduction.graded_columns().len() as u64).to_be_bytes());
            for columns in reduction.graded_columns() {
                digest_columns(hash, columns);
            }
            reduction.diagram()
        }
        InterfaceState::Composed { mode, diagram } => {
            hash.update([match mode {
                InterfaceMode::Relative => unreachable!("a composed state has a composed mode"),
                InterfaceMode::Materialized => unreachable!("a composed state has a composed mode"),
                InterfaceMode::Disjoint => 1,
                InterfaceMode::ZeroSimplex => 2,
                InterfaceMode::ZeroCone => 3,
            }]);
            diagram
        }
    }
}

fn digest_diagram(hash: &mut Sha256, diagram: &Diagram) {
    hash.update((diagram.bars.len() as u64).to_be_bytes());
    for bar in &diagram.bars {
        hash.update((bar.dim as u64).to_be_bytes());
        hash.update(bar.birth.to_bits().to_be_bytes());
        hash.update(bar.death.to_bits().to_be_bytes());
    }
}

fn digest_usizes(hash: &mut Sha256, values: &[usize]) {
    hash.update((values.len() as u64).to_be_bytes());
    for &value in values {
        hash.update((value as u64).to_be_bytes());
    }
}

fn digest_columns(hash: &mut Sha256, columns: &[crate::ChangeColumn]) {
    hash.update((columns.len() as u64).to_be_bytes());
    for column in columns {
        hash.update((column.terms.len() as u64).to_be_bytes());
        for term in &column.terms {
            hash.update((term.index as u64).to_be_bytes());
            hash.update(term.coefficient.to_be_bytes());
        }
    }
}

fn summarize(node: &Arc<InterfaceNode>, summary: &mut IndexSummary) {
    summary.nodes += 1;
    match node.mode() {
        InterfaceMode::Relative => {
            let relative = node.relative().expect("relative mode has a relative core");
            let work = relative.work();
            summary.relative_interfaces += 1;
            summary.composed_interfaces += usize::from(!node.children.is_empty());
            summary.relative_input_cells += work.input_cells;
            summary.relative_core_cells += work.core_cells;
            summary.relative_cancellations += work.cancellations;
            summary.largest_relative_core_cells =
                summary.largest_relative_core_cells.max(work.core_cells);
        }
        InterfaceMode::Materialized => {
            summary.materialized_interfaces += 1;
            summary.largest_interface_vertices =
                summary.largest_interface_vertices.max(node.vertices.len());
            summary.largest_interface_edges = summary
                .largest_interface_edges
                .max(node.edge_positions.len());
        }
        InterfaceMode::Disjoint | InterfaceMode::ZeroSimplex | InterfaceMode::ZeroCone => {
            summary.composed_interfaces += 1;
        }
    }
    if node.children.is_empty() {
        summary.leaves += 1;
    } else if node.separator.is_empty() {
        summary.component_splits += 1;
    } else {
        summary.separators += 1;
        summary.widest_separator = summary.widest_separator.max(node.separator.len());
    }
    for child in &node.children {
        summarize(child, summary);
    }
}

fn collect_interfaces(node: &Arc<InterfaceNode>, depth: usize, output: &mut Vec<InterfaceSummary>) {
    output.push(InterfaceSummary {
        digest: node.digest,
        depth,
        vertices: node.vertices.clone(),
        separator: node.separator.clone(),
        protected_vertices: node.protected_vertices.clone(),
        edges: node.edge_positions.len(),
        children: node.children.len(),
        mode: node.mode(),
        reduction_columns: node
            .reduction()
            .map(GradedReductionCertificate::column_count)
            .unwrap_or(0),
        columns_by_dimension: node
            .reduction()
            .map(|reduction| reduction.graded_columns().iter().map(Vec::len).collect())
            .unwrap_or_default(),
        relative_input_cells: node
            .relative()
            .map(|value| value.work().input_cells)
            .unwrap_or(0),
        relative_core_cells: node
            .relative()
            .map(|value| value.work().core_cells)
            .unwrap_or(0),
        relative_cancellations: node
            .relative()
            .map(|value| value.work().cancellations)
            .unwrap_or(0),
    });
    for child in &node.children {
        collect_interfaces(child, depth + 1, output);
    }
}

fn count_nodes(node: &Arc<InterfaceNode>) -> usize {
    1 + node.children.iter().map(count_nodes).sum::<usize>()
}

fn count_columns(node: &Arc<InterfaceNode>) -> usize {
    node.reduction()
        .map(GradedReductionCertificate::column_count)
        .or_else(|| {
            node.relative()
                .map(|relative| relative.graded_columns().iter().map(Vec::len).sum())
        })
        .unwrap_or(0)
        + node.children.iter().map(count_columns).sum::<usize>()
}

fn shared_nodes(left: &Arc<InterfaceNode>, right: &Arc<InterfaceNode>) -> usize {
    if Arc::ptr_eq(left, right) {
        return count_nodes(left);
    }
    left.children
        .iter()
        .zip(&right.children)
        .map(|(a, b)| shared_nodes(a, b))
        .sum()
}

fn correspondences(
    old: &PersistenceIndex,
    new: &PersistenceIndex,
    mode: CorrespondenceMode,
) -> Result<Vec<ClassCorrespondence>> {
    if mode == CorrespondenceMode::Omit {
        return Ok(Vec::new());
    }
    let old_explained = old.explain()?;
    let new_explained = new.explain()?;
    class_correspondences(
        &old.graph,
        &old_explained.spaces,
        &new.graph,
        &new_explained.spaces,
        old.params.modulus,
    )
}

fn diagram_delta(old: &Diagram, new: &Diagram) -> DiagramDelta {
    type Key = (usize, u64, u64);
    let mut old_counts = BTreeMap::<Key, usize>::new();
    let mut new_counts = BTreeMap::<Key, usize>::new();
    for bar in &old.bars {
        *old_counts
            .entry((bar.dim, bar.birth.to_bits(), bar.death.to_bits()))
            .or_default() += 1;
    }
    for bar in &new.bars {
        *new_counts
            .entry((bar.dim, bar.birth.to_bits(), bar.death.to_bits()))
            .or_default() += 1;
    }
    let keys: BTreeSet<_> = old_counts
        .keys()
        .chain(new_counts.keys())
        .copied()
        .collect();
    let mut delta = DiagramDelta::default();
    for (dim, birth, death) in keys {
        let old_count = old_counts.get(&(dim, birth, death)).copied().unwrap_or(0);
        let new_count = new_counts.get(&(dim, birth, death)).copied().unwrap_or(0);
        delta.removed.extend((new_count..old_count).map(|_| Bar {
            dim,
            birth: f64::from_bits(birth),
            death: f64::from_bits(death),
        }));
        delta.added.extend((old_count..new_count).map(|_| Bar {
            dim,
            birth: f64::from_bits(birth),
            death: f64::from_bits(death),
        }));
    }
    delta
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rips_persistence_sparse;

    fn shared_edge_graph(separator_weight: f64) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, separator_weight),
                (0, 2, 1.0),
                (1, 2, 1.5),
                (0, 3, 1.2),
                (1, 3, 1.7),
                (0, 4, 1.1),
                (1, 4, 1.6),
                (0, 5, 1.3),
                (1, 5, 1.8),
            ],
        )
        .unwrap()
    }

    fn compose_index_params() -> IndexParams {
        IndexParams {
            interface_policy: InterfacePolicy::Compose,
            ..IndexParams::default()
        }
    }

    fn joined_octahedra(changed: bool) -> SparseDistanceMatrix {
        let atoms = [[0, 1, 2, 3, 4, 5], [0, 6, 7, 8, 9, 10]];
        let mut edges = Vec::new();
        for vertices in atoms {
            let opposite = [
                EdgeKey::new(vertices[0], vertices[1]),
                EdgeKey::new(vertices[2], vertices[3]),
                EdgeKey::new(vertices[4], vertices[5]),
            ];
            for left in 0..vertices.len() {
                for right in left + 1..vertices.len() {
                    let edge = EdgeKey::new(vertices[left], vertices[right]);
                    if !opposite.contains(&edge) {
                        let offset = if changed && edge == EdgeKey::new(0, 2) {
                            0.0001
                        } else {
                            0.0
                        };
                        edges.push((
                            edge.u,
                            edge.v,
                            1.0 + (edge.u + edge.v) as f64 / 100.0 + offset,
                        ));
                    }
                }
            }
        }
        edges.sort_by_key(|&(u, v, _)| (u, v));
        SparseDistanceMatrix::from_triplets(11, &edges).unwrap()
    }

    fn zero_cone_cover(nonzero_cone_edge: bool) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            5,
            &[
                (0, 1, if nonzero_cone_edge { 0.1 } else { 0.0 }),
                (0, 2, 0.0),
                (0, 3, 1.0),
                (1, 3, 1.1),
                (2, 3, 1.2),
                (0, 4, 1.3),
                (1, 4, 1.4),
                (2, 4, 1.5),
            ],
        )
        .unwrap()
    }

    #[test]
    fn zero_cone_interface_is_dimension_generic_and_checked_by_fallback() {
        let graph = zero_cone_cover(false);
        let index_params = IndexParams {
            max_separator_width: 3,
            leaf_vertices: 4,
            ..compose_index_params()
        };
        for max_dim in 0..=3 {
            let params = RipsParams::new(max_dim).with_modulus(3);
            let composed = PersistenceIndex::compile(
                &graph,
                &params,
                index_params,
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(composed.interfaces()[0].mode, InterfaceMode::ZeroCone);
            assert_eq!(
                composed.diagram().bars,
                rips_persistence_sparse(&graph, &params).unwrap().bars
            );
            let materialized = PersistenceIndex::compile(
                &graph,
                &params,
                IndexParams {
                    interface_policy: InterfacePolicy::Materialize,
                    ..index_params
                },
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(composed.diagram().bars, materialized.diagram().bars);
        }

        let params = RipsParams::new(2).with_modulus(3);
        let nonzero = PersistenceIndex::compile(
            &zero_cone_cover(true),
            &params,
            index_params,
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(nonzero.interfaces()[0].mode, InterfaceMode::Materialized);

        let composed =
            PersistenceIndex::compile(&graph, &params, index_params, CertificateLimits::default())
                .unwrap();
        let materialized = composed.transition(&zero_cone_cover(true)).unwrap();
        assert_eq!(
            materialized.index.interfaces()[0].mode,
            InterfaceMode::Materialized
        );
        assert_eq!(materialized.mode, IndexUpdateMode::Rebuilt);
        let restored = materialized.index.transition(&graph).unwrap();
        assert_eq!(restored.index.interfaces()[0].mode, InterfaceMode::ZeroCone);
        assert!(restored.work.nodes_composed > 0);
    }

    #[test]
    fn h2_composes_across_a_zero_vertex_and_repairs_one_route() {
        let initial = joined_octahedra(false);
        let updated = joined_octahedra(true);
        let index_params = IndexParams {
            max_separator_width: 1,
            leaf_vertices: 6,
            ..compose_index_params()
        };
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(2).with_modulus(modulus);
            let first = PersistenceIndex::compile(
                &initial,
                &params,
                index_params,
                CertificateLimits::default(),
            )
            .unwrap();
            assert!(first.summary().root_composed);
            assert_eq!(first.diagram().in_dim(2).count(), 2);
            assert_eq!(
                first.diagram().bars,
                rips_persistence_sparse(&initial, &params).unwrap().bars
            );
            let transition = first.transition(&updated).unwrap();
            assert!(transition.work.nodes_shared > 0);
            assert!(transition.work.nodes_composed > 0);
            assert_eq!(
                transition.index.diagram().bars,
                rips_persistence_sparse(&updated, &params).unwrap().bars
            );
        }
    }

    #[test]
    fn nonzero_separator_has_an_exact_root_interface() {
        let graph = shared_edge_graph(0.25);
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let index = PersistenceIndex::compile(
                &graph,
                &params,
                compose_index_params(),
                CertificateLimits::default(),
            )
            .unwrap();
            let expected = rips_persistence_sparse(&graph, &params).unwrap();
            assert_eq!(index.diagram().bars, expected.bars);
            assert!(index.summary().separators > 0);
            assert!(!index.summary().root_composed);
            assert!(
                index
                    .interfaces()
                    .iter()
                    .any(|interface| interface.separator == [0, 1])
            );
        }
    }

    #[test]
    fn zero_simplex_separator_omits_the_global_reduction() {
        let graph = shared_edge_graph(0.0);
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let index = PersistenceIndex::compile(
                &graph,
                &params,
                compose_index_params(),
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(
                index.diagram().bars,
                rips_persistence_sparse(&graph, &params).unwrap().bars
            );
            assert!(index.summary().root_composed);
            assert!(index.summary().composed_interfaces > 0);
            assert_eq!(index.interfaces()[0].mode, InterfaceMode::ZeroSimplex);
            assert_eq!(index.interfaces()[0].reduction_columns, 0);

            let baseline_params = IndexParams {
                interface_policy: InterfacePolicy::Materialize,
                ..IndexParams::default()
            };
            let baseline = PersistenceIndex::compile(
                &graph,
                &params,
                baseline_params,
                CertificateLimits::default(),
            )
            .unwrap();
            assert_eq!(baseline.diagram().bars, index.diagram().bars);
            assert!(!baseline.summary().root_composed);
            assert_eq!(baseline.summary().composed_interfaces, 0);
        }
    }

    #[test]
    fn a_separator_edit_switches_between_composed_and_materialized_states() {
        let initial = shared_edge_graph(0.0);
        let updated = shared_edge_graph(0.25);
        let params = RipsParams::new(1).with_modulus(3);
        let first = PersistenceIndex::compile(
            &initial,
            &params,
            compose_index_params(),
            CertificateLimits::default(),
        )
        .unwrap();
        let second = first.transition(&updated).unwrap();
        assert!(first.summary().root_composed);
        assert!(!second.index.summary().root_composed);
        assert_eq!(second.mode, IndexUpdateMode::Rebuilt);
        assert_eq!(
            second.index.diagram().bars,
            rips_persistence_sparse(&updated, &params).unwrap().bars
        );
        let third = second.index.transition(&initial).unwrap();
        assert!(third.index.summary().root_composed);
        assert!(third.work.nodes_composed > 0);
        assert_eq!(
            third.index.diagram().bars,
            rips_persistence_sparse(&initial, &params).unwrap().bars
        );
    }

    #[test]
    fn local_transition_shares_untouched_subtrees() {
        let initial = shared_edge_graph(0.25);
        let updated = SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 0.25),
                (0, 2, 1.01),
                (1, 2, 1.5),
                (0, 3, 1.2),
                (1, 3, 1.7),
                (0, 4, 1.1),
                (1, 4, 1.6),
                (0, 5, 1.3),
                (1, 5, 1.8),
            ],
        )
        .unwrap();
        let params = RipsParams::new(1).with_modulus(3);
        let index = PersistenceIndex::compile(
            &initial,
            &params,
            compose_index_params(),
            CertificateLimits::default(),
        )
        .unwrap();
        let transition = index.transition(&updated).unwrap();
        let expected = rips_persistence_sparse(&updated, &params).unwrap();
        assert_eq!(transition.index.diagram().bars, expected.bars);
        assert!(transition.work.nodes_shared > 0);
        assert_eq!(
            index.shared_nodes_with(&transition.index),
            transition.work.nodes_shared
        );
    }

    #[test]
    fn threshold_crossing_keeps_the_tree_and_rebuilds_reductions() {
        let initial = shared_edge_graph(0.25);
        let updated = shared_edge_graph(2.5);
        let mut params = RipsParams::new(1).with_modulus(5);
        params.threshold = Some(2.0);
        let index = PersistenceIndex::compile(
            &initial,
            &params,
            compose_index_params(),
            CertificateLimits::default(),
        )
        .unwrap();
        let transition = index.transition(&updated).unwrap();
        assert_eq!(transition.mode, IndexUpdateMode::Rebuilt);
        assert!(
            transition
                .events
                .iter()
                .any(|event| event.kind == IndexEventKind::ThresholdCrossing)
        );
        assert_eq!(index.summary(), transition.index.summary());
        assert_eq!(
            transition.index.diagram().bars,
            rips_persistence_sparse(&updated, &params).unwrap().bars
        );
    }

    #[test]
    fn exact_relations_remain_opt_in() {
        let initial = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.0), (1, 2, 1.1), (2, 3, 1.2), (0, 3, 1.3)],
        )
        .unwrap();
        let updated = SparseDistanceMatrix::from_triplets(
            4,
            &[(0, 1, 1.01), (1, 2, 1.1), (2, 3, 1.2), (0, 3, 1.3)],
        )
        .unwrap();
        let params = RipsParams::new(1).with_modulus(3);
        let index = PersistenceIndex::compile(
            &initial,
            &params,
            compose_index_params(),
            CertificateLimits::default(),
        )
        .unwrap();
        let transition = index
            .transition_with(&updated, CorrespondenceMode::Exact)
            .unwrap();
        assert!(!transition.correspondence.is_empty());
    }

    #[test]
    fn edits_cross_the_threshold_without_changing_the_envelope() {
        let initial = shared_edge_graph(0.25);
        let mut params = RipsParams::new(1);
        params.threshold = Some(2.0);
        let index = PersistenceIndex::compile(
            &initial,
            &params,
            compose_index_params(),
            CertificateLimits::default(),
        )
        .unwrap();
        let transition = index
            .transition_edits(&[IndexEdit::deactivate(0, 1)])
            .unwrap();
        assert_eq!(transition.index.graph().num_edges(), initial.num_edges());
        assert_eq!(transition.work.edges_checked, 1);
        assert_eq!(transition.mode, IndexUpdateMode::Rebuilt);
        assert!(
            transition
                .events
                .iter()
                .any(|event| event.kind == IndexEventKind::ThresholdCrossing)
        );
    }

    #[test]
    fn batches_are_atomic_and_alternatives_keep_input_order() {
        let initial = shared_edge_graph(0.25);
        let first = SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 0.25),
                (0, 2, 1.01),
                (1, 2, 1.5),
                (0, 3, 1.2),
                (1, 3, 1.7),
                (0, 4, 1.1),
                (1, 4, 1.6),
                (0, 5, 1.3),
                (1, 5, 1.8),
            ],
        )
        .unwrap();
        let second = SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 0.25),
                (0, 2, 1.0),
                (1, 2, 1.5),
                (0, 3, 1.2),
                (1, 3, 1.7),
                (0, 4, 1.11),
                (1, 4, 1.6),
                (0, 5, 1.3),
                (1, 5, 1.8),
            ],
        )
        .unwrap();
        let invalid_triplets: Vec<_> = (0..6)
            .flat_map(|u| (u + 1..6).map(move |v| (u, v, 1.0 + (u + v) as f64 / 100.0)))
            .collect();
        let invalid = SparseDistanceMatrix::from_triplets(6, &invalid_triplets).unwrap();
        let mut params = RipsParams::new(1);
        params.threads = 2;
        let limits = CertificateLimits {
            max_triangles: 4,
            ..CertificateLimits::default()
        };
        let mut index =
            PersistenceIndex::compile(&initial, &params, IndexParams::default(), limits).unwrap();
        let original = index.version();
        assert!(index.advance_batch(&[first.clone(), invalid]).is_err());
        assert_eq!(index.version(), original);

        let branches = index.branch(&[first, second]).unwrap();
        assert_eq!(branches.len(), 2);
        assert_eq!(branches[0].index, 0);
        assert_eq!(branches[1].index, 1);
        assert_eq!(index.version(), original);
        assert!(
            branches
                .iter()
                .all(|branch| branch.transition.work.nodes_shared > 0)
        );
    }

    #[test]
    fn disconnected_envelope_recompiles_after_a_bridge_is_added() {
        let initial = SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 1.0),
                (0, 2, 1.1),
                (1, 2, 1.2),
                (3, 4, 1.3),
                (3, 5, 1.4),
                (4, 5, 1.5),
            ],
        )
        .unwrap();
        let updated = SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 1.0),
                (0, 2, 1.1),
                (1, 2, 1.2),
                (2, 3, 1.25),
                (3, 4, 1.3),
                (3, 5, 1.4),
                (4, 5, 1.5),
            ],
        )
        .unwrap();
        let params = RipsParams::new(1).with_modulus(3);
        let index = PersistenceIndex::compile(
            &initial,
            &params,
            IndexParams::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(index.summary().component_splits, 1);
        let transition = index.transition(&updated).unwrap();
        assert_eq!(transition.mode, IndexUpdateMode::Recompiled);
        assert!(
            transition
                .events
                .iter()
                .any(|event| event.kind == IndexEventKind::EnvelopeRecompiled)
        );
        let diff = index.diff(&transition.index);
        assert!(!diff.same_envelope);
        assert_eq!(
            transition.index.diagram().bars,
            rips_persistence_sparse(&updated, &params).unwrap().bars
        );
    }

    #[test]
    fn random_fixed_envelope_versions_match_clean_reduction() {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        for case in 0..48 {
            let vertices = 4 + case % 5;
            let mut initial_triplets = Vec::new();
            let mut updated_triplets = Vec::new();
            for v in 1..vertices {
                for u in 0..v {
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    if state % 4 == 0 {
                        continue;
                    }
                    let first = 0.25 * (1 + state % 12) as f64;
                    state ^= state << 13;
                    state ^= state >> 7;
                    state ^= state << 17;
                    let second = 0.25 * (1 + state % 12) as f64;
                    initial_triplets.push((u, v, first));
                    updated_triplets.push((u, v, second));
                }
            }
            let initial = SparseDistanceMatrix::from_triplets(vertices, &initial_triplets).unwrap();
            let updated = SparseDistanceMatrix::from_triplets(vertices, &updated_triplets).unwrap();
            for modulus in [2, 3, 5] {
                let mut params = RipsParams::new(case % 3).with_modulus(modulus);
                params.threshold = (case % 2 == 0).then_some(2.0);
                let index = PersistenceIndex::compile(
                    &initial,
                    &params,
                    IndexParams::default(),
                    CertificateLimits::default(),
                )
                .unwrap();
                assert_eq!(
                    index.diagram().bars,
                    rips_persistence_sparse(&initial, &params).unwrap().bars,
                    "initial case {case}, modulus {modulus}, edges {:?}, interfaces {:?}",
                    initial.edges().collect::<Vec<_>>(),
                    index.interfaces(),
                );
                let transition = index.transition(&updated).unwrap();
                assert_eq!(
                    transition.index.diagram().bars,
                    rips_persistence_sparse(&updated, &params).unwrap().bars,
                    "updated case {case}, modulus {modulus}, edges {:?}, interfaces {:?}",
                    updated.edges().collect::<Vec<_>>(),
                    transition.index.interfaces(),
                );
            }
        }
    }

    #[test]
    fn relative_index_keeps_ancestor_separators_and_updates_one_route() {
        let graph = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 0.75),
                (0, 3, 0.5),
                (0, 5, 2.5),
                (0, 6, 1.75),
                (1, 2, 3.0),
                (1, 3, 2.5),
                (1, 4, 1.5),
                (1, 5, 3.0),
                (2, 3, 1.0),
                (2, 4, 0.75),
                (2, 5, 0.5),
                (2, 6, 0.5),
                (3, 4, 2.75),
                (3, 5, 2.75),
                (4, 5, 3.0),
                (4, 6, 0.5),
                (5, 6, 2.0),
            ],
        )
        .unwrap();
        let mut params = RipsParams::new(2).with_modulus(3);
        params.threshold = Some(2.0);
        let index = PersistenceIndex::compile(
            &graph,
            &params,
            IndexParams::default(),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(index.interfaces()[0].mode, InterfaceMode::Relative);
        assert_eq!(
            index.diagram().bars,
            rips_persistence_sparse(&graph, &params).unwrap().bars
        );
        let root_separator = index.interfaces()[0].separator.clone();
        for interface in index.interfaces().into_iter().skip(1) {
            for vertex in &root_separator {
                if interface.vertices.contains(vertex) {
                    assert!(interface.protected_vertices.contains(vertex));
                }
            }
        }

        let transition = index
            .transition_edits(&[IndexEdit::set_weight(0, 1, 0.8)])
            .unwrap();
        assert_eq!(transition.mode, IndexUpdateMode::Relative);
        assert!(transition.work.relative_nodes_rebuilt > 0);
        assert!(transition.work.relative_nodes_composed > 0);
        assert!(transition.work.nodes_shared > 0);
        assert_eq!(
            transition.index.diagram().bars,
            rips_persistence_sparse(transition.index.graph(), &params)
                .unwrap()
                .bars
        );
    }
}
