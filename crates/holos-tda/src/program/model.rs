use crate::{
    AtlasArtifact, BasisClassId, CertificateLimits, ClassCorrespondence, Diagram, EdgeKey,
    ExplainedDiagram, IntervalGroupId, ReductionGuardKind, Result, RipsParams,
    SparseDistanceMatrix,
};

/// One atom in a compiled sparse persistence program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramAtomInfo {
    /// Stable position in the program decomposition.
    pub id: usize,
    /// Original labeled vertices in ascending order.
    pub vertices: Vec<usize>,
    /// Original labeled edges in ascending endpoint order.
    pub edges: Vec<EdgeKey>,
    /// Vertices shared with another atom.
    pub separator_vertices: Vec<usize>,
    /// Whether the atom can contain positive-dimensional homology.
    pub cyclic: bool,
}

/// Structural and algebraic size of a compiled program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramSummary {
    /// Vertex-biconnected atoms, including bridge atoms.
    pub atoms: usize,
    /// Atoms that contain a graph cycle.
    pub cyclic_atoms: usize,
    /// Distinct articulation vertices.
    pub articulation_vertices: usize,
    /// Wider zero-filtration simplex separators used by the program.
    pub zero_simplex_separators: usize,
    /// Largest separator used by the program.
    pub widest_separator: usize,
    /// Candidate vertex sets checked during bounded separator search.
    pub separator_candidates_checked: usize,
    /// Whether bounded separator search visited every candidate in scope.
    pub separator_search_complete: bool,
    /// Edges in the largest cyclic atom.
    pub largest_cyclic_atom_edges: usize,
    /// Distinct reduction guards before transitive removal.
    pub complete_guards: usize,
    /// Result-sensitive guards across all cyclic atoms.
    pub guards: usize,
}

/// Exact work charged to one program evaluation or update.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProgramWork {
    /// Listed edges compared with the current program input.
    pub edges_checked: usize,
    /// Edges read by the H0 computation.
    pub h0_edges_scanned: usize,
    /// Result-sensitive algebraic guards checked.
    pub guards_checked: usize,
    /// Cyclic atoms containing a changed edge.
    pub atoms_touched: usize,
    /// Touched atoms reused without reduction.
    pub atoms_reused: usize,
    /// Touched atoms repaired by reducing a suffix.
    pub atoms_repaired: usize,
    /// Touched atoms whose explained state was rebuilt without suffix repair.
    pub atoms_rebuilt: usize,
    /// Boundary columns retained during reduction repair.
    pub reduction_columns_reused: usize,
    /// Boundary columns processed during reduction repair.
    pub reduction_columns_reduced: usize,
    /// Sparse column additions performed during reduction repair.
    pub reduction_column_additions: usize,
}

/// Why a program update changed execution state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgramEventKind {
    /// The labeled vertex set changed.
    VertexSetChanged,
    /// The complete listed edge set changed.
    EdgeSetChanged,
    /// An edge crossed the fixed threshold.
    ThresholdCrossing,
    /// A result-sensitive algebraic guard failed.
    GuardFailed,
    /// A touched cyclic atom rebuilt its explained state.
    AtomRebuilt,
    /// A touched cyclic atom repaired a reduction suffix.
    ReductionSuffixRepaired,
    /// A wider separator stopped being a zero-filtration simplex.
    SeparatorContractChanged,
}

/// One event reported by a program update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramEvent {
    /// Kind of change.
    pub kind: ProgramEventKind,
    /// Affected atom, when one exists.
    pub atom: Option<usize>,
    /// Affected edge, when one exists.
    pub edge: Option<EdgeKey>,
    /// Failed guard kind, when one exists.
    pub guard: Option<ReductionGuardKind>,
}

/// How a program produced an updated result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramUpdateMode {
    /// All touched atoms retained their checked reductions.
    Reused,
    /// At least one touched atom was repaired or rebuilt locally.
    Repaired,
    /// The graph topology or threshold membership changed.
    Recompiled,
}

/// Control over exact relations to the preceding class spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum CorrespondenceMode {
    /// Return exact relations on every common filtered subcomplex.
    #[default]
    Exact,
    /// Keep the exact current state. Leave correspondence empty.
    Omit,
}

/// Algebraic relation between class spaces across one update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ContinuationKind {
    /// One old space maps bijectively to one new space.
    Isomorphism,
    /// One old space contributes to several new spaces.
    Split,
    /// Several old spaces contribute to one new space.
    Merge,
    /// Several old and new spaces share exact basis vectors.
    Mixing,
    /// A new space has no exact old basis vector.
    Birth,
    /// An old space has no exact new basis vector.
    Death,
    /// A proper subset of the vectors has an exact declared continuation.
    Ambiguous,
}

/// One exact equality between an old and new canonical basis vector.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BasisTransport {
    /// Old basis identifier.
    pub old: BasisClassId,
    /// New basis identifier.
    pub new: BasisClassId,
    /// Nonzero coefficient in the shared prime field.
    pub coefficient: u32,
}

/// One path-relative class-space continuation record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassContinuation {
    /// Algebraic shape of the relation.
    pub kind: ContinuationKind,
    /// Old class spaces in the connected relation component.
    pub old_spaces: Vec<IntervalGroupId>,
    /// New class spaces in the connected relation component.
    pub new_spaces: Vec<IntervalGroupId>,
    /// Exact shared canonical basis vectors.
    pub transport: Vec<BasisTransport>,
}

/// Diagram-only evaluation from unchanged program certificates.
#[derive(Debug, Clone)]
pub struct ProgramEvaluation {
    /// Exact H0 and H1 diagram.
    pub diagram: Diagram,
    /// Work charged to the evaluation.
    pub work: ProgramWork,
}

/// How a diagram-only state produced an updated diagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgramDiagramUpdateMode {
    /// Existing checked regions produced the updated diagram.
    Reused,
    /// A complete compositional program was compiled as the exact fallback.
    Recompiled,
}

/// Result of advancing a diagram-only program state.
#[derive(Debug, Clone)]
pub struct ProgramDiagramUpdate {
    /// Exact H0 and H1 diagram at the new input.
    pub diagram: Diagram,
    /// How the diagram was produced.
    pub mode: ProgramDiagramUpdateMode,
    /// Topology or region events encountered during the update.
    pub events: Vec<ProgramEvent>,
    /// Exact work charged to the update.
    pub work: ProgramWork,
}

/// Result of advancing a persistence program.
#[derive(Debug, Clone)]
pub struct ProgramUpdate {
    /// Exact diagram and canonical H1 class spaces at the new input.
    pub result: ExplainedDiagram,
    /// How the program produced this result.
    pub mode: ProgramUpdateMode,
    /// Events encountered during the update.
    pub events: Vec<ProgramEvent>,
    /// Algebraic class-space continuation records.
    pub continuation: Vec<ClassContinuation>,
    /// Exact linear relations on common filtered subcomplexes.
    pub correspondence: Vec<ClassCorrespondence>,
    /// Exact work charged to the update.
    pub work: ProgramWork,
}

/// Immutable state from which a program can be restored or branched.
#[derive(Debug, Clone)]
pub struct ProgramCheckpoint {
    pub(super) program: PersistenceProgram,
}

impl ProgramCheckpoint {
    /// Program state stored by this checkpoint.
    pub fn program(&self) -> &PersistenceProgram {
        &self.program
    }

    /// Advance independent alternatives from this checkpoint.
    pub fn branch(&self, alternatives: &[SparseDistanceMatrix]) -> Result<Vec<ProgramBranch>> {
        self.program.branch(alternatives)
    }

    /// Advance alternatives with explicit correspondence control.
    pub fn branch_with(
        &self,
        alternatives: &[SparseDistanceMatrix],
        correspondence_mode: CorrespondenceMode,
    ) -> Result<Vec<ProgramBranch>> {
        self.program.branch_with(alternatives, correspondence_mode)
    }
}

/// One independently advanced program branch.
#[derive(Debug, Clone)]
pub struct ProgramBranch {
    /// Position of the alternative in the input batch.
    pub index: usize,
    /// Exact update from the shared branch point.
    pub update: ProgramUpdate,
    pub(super) program: PersistenceProgram,
}

/// Stateful diagram evaluation with lazy materialization of class spaces.
#[derive(Debug, Clone)]
pub struct ProgramDiagramState {
    pub(super) program: PersistenceProgram,
    pub(super) graph: SparseDistanceMatrix,
    pub(super) diagram: Diagram,
    pub(super) dirty: bool,
}

impl ProgramBranch {
    /// Program at the end of this branch.
    pub fn program(&self) -> &PersistenceProgram {
        &self.program
    }

    /// Consume this branch and return its program.
    pub fn into_program(self) -> PersistenceProgram {
        self.program
    }

    /// Consume this branch and return its update and program.
    pub fn into_parts(self) -> (ProgramUpdate, PersistenceProgram) {
        (self.update, self.program)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct ProgramAtomState {
    pub(crate) info_index: usize,
    pub(crate) vertices: Vec<usize>,
    pub(crate) edges: Vec<EdgeKey>,
    pub(crate) edge_positions: Vec<usize>,
    pub(crate) artifact: AtlasArtifact,
    pub(crate) certified_graph: SparseDistanceMatrix,
    pub(crate) region: crate::CertifiedReductionRegion,
    pub(crate) explained: ExplainedDiagram,
}

/// Exact H0 and H1 program compiled over sparse graph atoms.
#[derive(Debug, Clone)]
pub struct PersistenceProgram {
    pub(super) params: RipsParams,
    pub(super) limits: CertificateLimits,
    pub(super) graph: SparseDistanceMatrix,
    pub(super) topology: Vec<EdgeKey>,
    pub(super) active: Vec<bool>,
    pub(super) separator_edges: Vec<EdgeKey>,
    pub(super) h0_deaths: Vec<EdgeKey>,
    pub(super) h0_essential: usize,
    pub(super) atoms: Vec<ProgramAtomInfo>,
    pub(super) states: Vec<ProgramAtomState>,
    pub(super) summary: ProgramSummary,
    pub(super) result: ExplainedDiagram,
}
