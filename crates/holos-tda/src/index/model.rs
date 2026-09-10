use std::sync::Arc;

use crate::{
    Bar, CertificateLimits, ClassCorrespondence, Diagram, EdgeKey, GradedReductionCertificate,
    RelativeInterfaceCertificate, RipsParams, SparseDistanceMatrix,
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

/// Policy for parent interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterfacePolicy {
    /// Compose relative cores through arbitrary protected separators.
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

    pub(crate) fn edge(self) -> EdgeKey {
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
    /// Transition from the shared branch point.
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

/// An immutable, versioned persistence index.
///
/// The listed vertices and edges form an envelope. Weight changes and
/// threshold crossings inside that envelope preserve the separator tree.
/// A different listed-edge set compiles a new tree and reports that event.
#[derive(Debug, Clone)]
pub struct PersistenceIndex {
    pub(crate) params: RipsParams,
    pub(crate) index_params: IndexParams,
    pub(crate) limits: CertificateLimits,
    pub(crate) graph: Arc<SparseDistanceMatrix>,
    pub(crate) topology: Arc<Vec<EdgeKey>>,
    pub(crate) root: Arc<InterfaceNode>,
    pub(crate) summary: IndexSummary,
}
