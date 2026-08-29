//! Compositional, change-sensitive H0 and H1 persistence programs.
//!
//! A program splits positive-dimensional persistence at articulation
//! separators. It compiles each cyclic block into an independently checked
//! reduction region and computes H0 on the complete active graph. An update
//! rebuilds only cyclic blocks touched by changed weights. A topology or
//! threshold-membership change rebuilds the complete program.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::classes::{basis_class_id, canonical_space_basis, group_id, validate_h1_cocycle};
use crate::factorization::{
    FactorizationSummary, ProgramBlock, ProgramDecompositionSummary, program_blocks,
};
use crate::{
    AtlasArtifact, Bar, BasisClassId, CertificateLimits, ClassCorrespondence, Cocycle, CocycleTerm,
    CriticalPair, Diagram, EdgeKey, Error, ExplainedDiagram, IntervalGroupId, PersistentClass,
    PersistentClassSpace, ReductionGuardKind, RegionViolationKind, Result, RipsParams,
    SparseDistanceMatrix, rips_persistence_sparse,
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
    /// Touched atoms repaired from a retained reduction prefix.
    pub atoms_repaired: usize,
    /// Touched atoms rebuilt without a retained reduction prefix.
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
    /// A touched cyclic atom required exact reduction.
    AtomRebuilt,
    /// A touched cyclic atom retained a reduction prefix.
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

/// One checked, path-relative class-space continuation record.
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

/// Result of advancing a persistence program.
#[derive(Debug, Clone)]
pub struct ProgramUpdate {
    /// Exact diagram and canonical H1 class spaces at the new input.
    pub result: ExplainedDiagram,
    /// Whether the update reused, repaired, or recompiled state.
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
    program: PersistenceProgram,
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
    program: PersistenceProgram,
}

impl ProgramBranch {
    /// Checked program at the end of this branch.
    pub fn program(&self) -> &PersistenceProgram {
        &self.program
    }

    /// Consume this branch and return its checked program.
    pub fn into_program(self) -> PersistenceProgram {
        self.program
    }

    /// Consume this branch and return its update and checked program.
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

/// Exact H0 and H1 program compiled over checked sparse graph atoms.
#[derive(Debug, Clone)]
pub struct PersistenceProgram {
    params: RipsParams,
    limits: CertificateLimits,
    graph: SparseDistanceMatrix,
    topology: Vec<EdgeKey>,
    active: Vec<bool>,
    separator_edges: Vec<EdgeKey>,
    h0_deaths: Vec<EdgeKey>,
    h0_essential: usize,
    atoms: Vec<ProgramAtomInfo>,
    states: Vec<ProgramAtomState>,
    summary: ProgramSummary,
    result: ExplainedDiagram,
}

impl PersistenceProgram {
    /// Compile a checked compositional H0 and H1 program.
    ///
    /// Each cyclic atom receives its own reduction certificate. Graphs
    /// without a useful split remain one exact atom.
    pub fn compile(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
    ) -> Result<Self> {
        if params.max_dim != 1 {
            return Err(Error::InvalidInput(
                "a persistence program requires max_dim equal to 1".into(),
            ));
        }
        let (factorization, blocks) = program_blocks(input, params.threshold)?;
        let (atoms, states) = compile_atoms(input, params, limits, &blocks)?;
        let result = compose_result(input, params, &states)?;
        let expected = rips_persistence_sparse(input, params)?;
        if !diagram_bits_equal(&result.diagram, &expected) {
            return Err(Error::InvalidInput(format!(
                "compositional and monolithic diagrams differ: expected {:?}, got {:?}",
                expected.bars, result.diagram.bars
            )));
        }
        let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        let threshold = params.threshold.unwrap_or(f64::INFINITY);
        let active = input
            .edges()
            .map(|(_, _, value)| value <= threshold)
            .collect();
        let summary = program_summary(factorization, &atoms, &states);
        let separator_edges = wider_separator_edges(&atoms);
        let (h0_deaths, h0_essential, _) = h0_provenance(input, params.threshold);
        Ok(Self {
            params: params.clone(),
            limits,
            graph: input.clone(),
            topology,
            active,
            separator_edges,
            h0_deaths,
            h0_essential,
            atoms,
            states,
            summary,
            result,
        })
    }

    /// Exact current result.
    pub fn result(&self) -> &ExplainedDiagram {
        &self.result
    }

    /// Program decomposition and guard counts.
    pub fn summary(&self) -> ProgramSummary {
        self.summary
    }

    /// All structural atoms, including bridge atoms.
    pub fn atoms(&self) -> &[ProgramAtomInfo] {
        &self.atoms
    }

    /// Capture the complete checked state for later restore or branching.
    pub fn checkpoint(&self) -> ProgramCheckpoint {
        ProgramCheckpoint {
            program: self.clone(),
        }
    }

    /// Replace this program with a captured checked state.
    pub fn restore(&mut self, checkpoint: &ProgramCheckpoint) {
        *self = checkpoint.program.clone();
    }

    /// Advance an ordered batch atomically.
    ///
    /// If one update fails, the program is left unchanged.
    pub fn advance_batch(
        &mut self,
        updates: &[SparseDistanceMatrix],
    ) -> Result<Vec<ProgramUpdate>> {
        self.advance_batch_with(updates, CorrespondenceMode::Exact)
    }

    /// Advance an ordered batch atomically with correspondence control.
    pub fn advance_batch_with(
        &mut self,
        updates: &[SparseDistanceMatrix],
        correspondence_mode: CorrespondenceMode,
    ) -> Result<Vec<ProgramUpdate>> {
        let mut candidate = self.clone();
        let mut results = Vec::with_capacity(updates.len());
        for update in updates {
            results.push(candidate.advance_with(update, correspondence_mode)?);
        }
        *self = candidate;
        Ok(results)
    }

    /// Advance independent alternatives from the current state.
    ///
    /// Output order matches input order. The program is left unchanged. With
    /// more than one configured thread, alternatives run concurrently.
    pub fn branch(&self, alternatives: &[SparseDistanceMatrix]) -> Result<Vec<ProgramBranch>> {
        self.branch_with(alternatives, CorrespondenceMode::Exact)
    }

    /// Advance independent alternatives with correspondence control.
    pub fn branch_with(
        &self,
        alternatives: &[SparseDistanceMatrix],
        correspondence_mode: CorrespondenceMode,
    ) -> Result<Vec<ProgramBranch>> {
        if alternatives.is_empty() {
            return Ok(Vec::new());
        }
        if self.params.threads <= 1 || alternatives.len() == 1 {
            return alternatives
                .iter()
                .enumerate()
                .map(|(index, alternative)| {
                    let mut program = self.clone();
                    let update = program.advance_with(alternative, correspondence_mode)?;
                    Ok(ProgramBranch {
                        index,
                        update,
                        program,
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
                Error::InvalidInput(format!("cannot create branch workers: {error}"))
            })?;
        let results = pool.install(|| {
            alternatives
                .par_iter()
                .enumerate()
                .map(|(index, alternative)| {
                    let mut program = self.clone();
                    let update = program.advance_with(alternative, correspondence_mode)?;
                    Ok(ProgramBranch {
                        index,
                        update,
                        program,
                    })
                })
                .collect::<Vec<Result<ProgramBranch>>>()
        });
        results.into_iter().collect()
    }

    /// Evaluate only the diagram while every touched certificate remains
    /// valid.
    pub fn evaluate_diagram(&self, updated: &SparseDistanceMatrix) -> Result<ProgramEvaluation> {
        let mut work = ProgramWork {
            edges_checked: self.topology.len(),
            ..ProgramWork::default()
        };
        let edge_values = check_program_topology(self, updated)?;
        let mut h1 = Vec::new();
        for state in &self.states {
            h1.extend(
                state
                    .region
                    .evaluate_h1_indexed(&edge_values, &state.edge_positions)?,
            );
            work.guards_checked += state.region.guards().len();
        }
        let mut diagram = Diagram::default();
        for edge in &self.h0_deaths {
            let death = updated.get(edge.u, edge.v);
            if death > 0.0 {
                diagram.bars.push(Bar {
                    dim: 0,
                    birth: 0.0,
                    death,
                });
            }
        }
        diagram.bars.extend((0..self.h0_essential).map(|_| Bar {
            dim: 0,
            birth: 0.0,
            death: f64::INFINITY,
        }));
        work.h0_edges_scanned = self.h0_deaths.len();
        diagram.bars.extend(h1);
        diagram.canonicalize();
        Ok(ProgramEvaluation { diagram, work })
    }

    /// Advance to new weights with exact class correspondence.
    pub fn advance(&mut self, updated: &SparseDistanceMatrix) -> Result<ProgramUpdate> {
        self.advance_with(updated, CorrespondenceMode::Exact)
    }

    /// Advance with explicit control over cross-state class correspondence.
    pub fn advance_with(
        &mut self,
        updated: &SparseDistanceMatrix,
        correspondence_mode: CorrespondenceMode,
    ) -> Result<ProgramUpdate> {
        let old = self.result.clone();
        let old_graph = self.graph.clone();
        let topology_events = program_topology_events(self, updated);
        if !topology_events.is_empty() {
            return self.recompile_update(
                updated,
                old,
                old_graph,
                correspondence_mode,
                topology_events,
            );
        }

        let changed = changed_edges(self, updated);
        let mut work = ProgramWork {
            edges_checked: self.topology.len(),
            ..ProgramWork::default()
        };
        let mut events = Vec::new();
        for state in &mut self.states {
            if !state.edges.iter().any(|edge| changed.contains(edge)) {
                continue;
            }
            update_atom_state(
                state,
                updated,
                &changed,
                self.params.modulus,
                self.limits,
                &mut work,
                &mut events,
            )?;
        }
        self.finish_weight_update(updated, old, old_graph, correspondence_mode, events, work)
    }

    fn recompile_update(
        &mut self,
        updated: &SparseDistanceMatrix,
        old: ExplainedDiagram,
        old_graph: SparseDistanceMatrix,
        correspondence_mode: CorrespondenceMode,
        events: Vec<ProgramEvent>,
    ) -> Result<ProgramUpdate> {
        let replacement = Self::compile(updated, &self.params, self.limits)?;
        let result = replacement.result.clone();
        let correspondence = update_correspondence(
            correspondence_mode,
            &old_graph,
            &old.spaces,
            updated,
            &result.spaces,
            self.params.modulus,
        )?;
        let work = recompile_work(self.topology.len(), &replacement);
        let continuation = class_continuation(&old.spaces, &result.spaces);
        *self = replacement;
        Ok(ProgramUpdate {
            result,
            mode: ProgramUpdateMode::Recompiled,
            events,
            continuation,
            correspondence,
            work,
        })
    }

    fn finish_weight_update(
        &mut self,
        updated: &SparseDistanceMatrix,
        old: ExplainedDiagram,
        old_graph: SparseDistanceMatrix,
        correspondence_mode: CorrespondenceMode,
        events: Vec<ProgramEvent>,
        mut work: ProgramWork,
    ) -> Result<ProgramUpdate> {
        self.graph = updated.clone();
        self.result = compose_result(updated, &self.params, &self.states)?;
        self.summary.guards = self
            .states
            .iter()
            .map(|state| state.region.guards().len())
            .sum();
        let (h0_deaths, h0_essential, scanned) = h0_provenance(updated, self.params.threshold);
        self.h0_deaths = h0_deaths;
        self.h0_essential = h0_essential;
        work.h0_edges_scanned = scanned;
        let continuation = class_continuation(&old.spaces, &self.result.spaces);
        let correspondence = update_correspondence(
            correspondence_mode,
            &old_graph,
            &old.spaces,
            updated,
            &self.result.spaces,
            self.params.modulus,
        )?;
        Ok(ProgramUpdate {
            result: self.result.clone(),
            mode: update_mode(&work),
            events,
            continuation,
            correspondence,
            work,
        })
    }

    pub(crate) fn advance_reused(
        &mut self,
        updated: &SparseDistanceMatrix,
    ) -> Result<ProgramUpdate> {
        check_program_topology(self, updated)?;
        let old = self.result.clone();
        let old_graph = self.graph.clone();
        let changed: BTreeSet<_> = self
            .topology
            .iter()
            .copied()
            .filter(|edge| {
                self.graph.get(edge.u, edge.v).to_bits() != updated.get(edge.u, edge.v).to_bits()
            })
            .collect();
        let mut work = ProgramWork {
            edges_checked: self.topology.len(),
            ..ProgramWork::default()
        };
        for state in &mut self.states {
            if !state.edges.iter().any(|edge| changed.contains(edge)) {
                continue;
            }
            work.atoms_touched += 1;
            work.guards_checked += state.region.guards().len();
            let local = local_matrix(&state.vertices, &state.edges, updated)?;
            let evaluation = state.region.evaluate(&local)?;
            let Some(explained) =
                reweight_explained(&local, &state.explained, &evaluation, self.params.modulus)?
            else {
                return Err(Error::InvalidInput(
                    "program class-space state requires a checked checkpoint".into(),
                ));
            };
            state.artifact = state
                .artifact
                .rebind(
                    &state.certified_graph,
                    &local,
                    explained.clone(),
                    self.limits,
                )
                .map_err(|error| Error::InvalidInput(error.to_string()))?;
            state.certified_graph = local;
            state.explained = explained;
            work.atoms_reused += 1;
        }
        self.graph = updated.clone();
        self.result = compose_result(updated, &self.params, &self.states)?;
        let (h0_deaths, h0_essential, scanned) = h0_provenance(updated, self.params.threshold);
        self.h0_deaths = h0_deaths;
        self.h0_essential = h0_essential;
        work.h0_edges_scanned = scanned;
        Ok(ProgramUpdate {
            result: self.result.clone(),
            mode: ProgramUpdateMode::Reused,
            events: Vec::new(),
            continuation: class_continuation(&old.spaces, &self.result.spaces),
            correspondence: crate::class_correspondences(
                &old_graph,
                &old.spaces,
                updated,
                &self.result.spaces,
                self.params.modulus,
            )?,
            work,
        })
    }

    pub(crate) fn preview_update(
        &self,
        updated: &SparseDistanceMatrix,
        replacement_cyclic_atoms: usize,
    ) -> Result<(ProgramUpdateMode, Vec<ProgramEvent>, ProgramWork)> {
        let topology_events = program_topology_events(self, updated);
        if !topology_events.is_empty() {
            return Ok((
                ProgramUpdateMode::Recompiled,
                topology_events,
                ProgramWork {
                    edges_checked: self.topology.len().max(updated.num_edges()),
                    h0_edges_scanned: updated
                        .edges()
                        .filter(|&(_, _, value)| {
                            value <= self.params.threshold.unwrap_or(f64::INFINITY)
                        })
                        .count(),
                    guards_checked: 0,
                    atoms_touched: replacement_cyclic_atoms,
                    atoms_reused: 0,
                    atoms_repaired: 0,
                    atoms_rebuilt: replacement_cyclic_atoms,
                    reduction_columns_reused: 0,
                    reduction_columns_reduced: 0,
                    reduction_column_additions: 0,
                },
            ));
        }
        let changed = changed_edges(self, updated);
        let mut events = Vec::new();
        let mut work = ProgramWork {
            edges_checked: self.topology.len(),
            ..ProgramWork::default()
        };
        for state in &self.states {
            if !state.edges.iter().any(|edge| changed.contains(edge)) {
                continue;
            }
            preview_atom_state(
                state,
                updated,
                &changed,
                self.params.modulus,
                &mut work,
                &mut events,
            )?;
        }
        work.h0_edges_scanned = updated
            .edges()
            .filter(|&(_, _, value)| value <= self.params.threshold.unwrap_or(f64::INFINITY))
            .count();
        Ok((update_mode(&work), events, work))
    }

    pub(crate) fn from_verified_parts(
        input: &SparseDistanceMatrix,
        params: &RipsParams,
        limits: CertificateLimits,
        atoms: Vec<ProgramAtomInfo>,
        states: Vec<ProgramAtomState>,
        result: ExplainedDiagram,
    ) -> Self {
        let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
        let threshold = params.threshold.unwrap_or(f64::INFINITY);
        let active = input
            .edges()
            .map(|(_, _, value)| value <= threshold)
            .collect();
        let factorization = FactorizationSummary {
            blocks: atoms.len(),
            cyclic_blocks: atoms.iter().filter(|atom| atom.cyclic).count(),
            bridge_edges: atoms.iter().filter(|atom| !atom.cyclic).count(),
            cyclic_edges: atoms
                .iter()
                .filter(|atom| atom.cyclic)
                .map(|atom| atom.edges.len())
                .sum(),
            largest_cyclic_block_edges: atoms
                .iter()
                .filter(|atom| atom.cyclic)
                .map(|atom| atom.edges.len())
                .max()
                .unwrap_or(0),
        };
        let (articulation_vertices, zero_simplex_separators, widest_separator) =
            separator_stats(&atoms);
        let summary = program_summary(
            ProgramDecompositionSummary {
                articulation: factorization,
                articulation_vertices,
                zero_simplex_separators,
                widest_separator,
                separator_candidates_checked: 0,
                separator_search_complete: true,
            },
            &atoms,
            &states,
        );
        let separator_edges = wider_separator_edges(&atoms);
        let (h0_deaths, h0_essential, _) = h0_provenance(input, params.threshold);
        Self {
            params: params.clone(),
            limits,
            graph: input.clone(),
            topology,
            active,
            separator_edges,
            h0_deaths,
            h0_essential,
            atoms,
            states,
            summary,
            result,
        }
    }

    pub(crate) fn states(&self) -> &[ProgramAtomState] {
        &self.states
    }

    pub(crate) fn params(&self) -> &RipsParams {
        &self.params
    }

    pub(crate) fn limits(&self) -> CertificateLimits {
        self.limits
    }

    pub(crate) fn current_graph(&self) -> &SparseDistanceMatrix {
        &self.graph
    }
}

fn changed_edges(
    program: &PersistenceProgram,
    updated: &SparseDistanceMatrix,
) -> BTreeSet<EdgeKey> {
    program
        .topology
        .iter()
        .copied()
        .filter(|edge| {
            program.graph.get(edge.u, edge.v).to_bits() != updated.get(edge.u, edge.v).to_bits()
        })
        .collect()
}

fn update_correspondence(
    mode: CorrespondenceMode,
    old_graph: &SparseDistanceMatrix,
    old_spaces: &[PersistentClassSpace],
    updated: &SparseDistanceMatrix,
    new_spaces: &[PersistentClassSpace],
    modulus: u32,
) -> Result<Vec<ClassCorrespondence>> {
    if mode == CorrespondenceMode::Omit {
        return Ok(Vec::new());
    }
    crate::class_correspondences(old_graph, old_spaces, updated, new_spaces, modulus)
}

fn recompile_work(old_edges: usize, replacement: &PersistenceProgram) -> ProgramWork {
    let threshold = replacement.params.threshold.unwrap_or(f64::INFINITY);
    ProgramWork {
        edges_checked: old_edges.max(replacement.topology.len()),
        h0_edges_scanned: replacement
            .graph
            .edges()
            .filter(|&(_, _, value)| value <= threshold)
            .count(),
        atoms_touched: replacement.states.len(),
        atoms_rebuilt: replacement.states.len(),
        reduction_columns_reduced: replacement
            .states
            .iter()
            .map(|state| {
                let certificate = state.artifact.reduction_certificate();
                certificate.edge_columns().len() + certificate.triangle_columns().len()
            })
            .sum(),
        ..ProgramWork::default()
    }
}

fn update_mode(work: &ProgramWork) -> ProgramUpdateMode {
    if work.atoms_rebuilt == 0 && work.atoms_repaired == 0 {
        ProgramUpdateMode::Reused
    } else {
        ProgramUpdateMode::Repaired
    }
}

fn violation_event(state: &ProgramAtomState, violation: &crate::RegionViolation) -> ProgramEvent {
    let kind = match violation.kind() {
        RegionViolationKind::VertexSetChanged => ProgramEventKind::VertexSetChanged,
        RegionViolationKind::EdgeSetChanged => ProgramEventKind::EdgeSetChanged,
        RegionViolationKind::ThresholdCrossing => ProgramEventKind::ThresholdCrossing,
        RegionViolationKind::GuardFailed => ProgramEventKind::GuardFailed,
    };
    ProgramEvent {
        kind,
        atom: Some(state.info_index),
        edge: violation.first().and_then(simplex_edge),
        guard: violation
            .guard_index()
            .map(|index| state.region.guards()[index].kind()),
    }
}

fn reusable_explanation(
    state: &ProgramAtomState,
    local: &SparseDistanceMatrix,
    modulus: u32,
    events: &mut Vec<ProgramEvent>,
) -> Result<Option<ExplainedDiagram>> {
    let violations = state.region.violations(local);
    if !violations.is_empty() {
        events.extend(
            violations
                .iter()
                .map(|violation| violation_event(state, violation)),
        );
        return Ok(None);
    }
    let evaluation = state.region.evaluate(local)?;
    reweight_explained(local, &state.explained, &evaluation, modulus)
}

fn update_atom_state(
    state: &mut ProgramAtomState,
    updated: &SparseDistanceMatrix,
    changed: &BTreeSet<EdgeKey>,
    modulus: u32,
    limits: CertificateLimits,
    work: &mut ProgramWork,
    events: &mut Vec<ProgramEvent>,
) -> Result<()> {
    work.atoms_touched += 1;
    work.guards_checked += state.region.guards().len();
    let local = local_matrix(&state.vertices, &state.edges, updated)?;
    if let Some(explained) = reusable_explanation(state, &local, modulus, events)? {
        install_reused_atom(state, local, explained, limits)?;
        work.atoms_reused += 1;
        return Ok(());
    }
    repair_atom_state(state, local, changed, limits, work, events)
}

fn install_reused_atom(
    state: &mut ProgramAtomState,
    local: SparseDistanceMatrix,
    explained: ExplainedDiagram,
    limits: CertificateLimits,
) -> Result<()> {
    state.artifact = state
        .artifact
        .rebind(&state.certified_graph, &local, explained.clone(), limits)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    state.certified_graph = local;
    state.explained = explained;
    Ok(())
}

fn repair_atom_state(
    state: &mut ProgramAtomState,
    local: SparseDistanceMatrix,
    changed: &BTreeSet<EdgeKey>,
    limits: CertificateLimits,
    work: &mut ProgramWork,
    events: &mut Vec<ProgramEvent>,
) -> Result<()> {
    let repair = state
        .artifact
        .repair(&state.certified_graph, &local, limits)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let repair_mode = repair.mode();
    let repair_work = repair.work();
    let artifact = repair.into_artifact();
    let region = artifact
        .reduction_certificate()
        .compile_region(&local, limits)?;
    state.explained = artifact.explained().clone();
    state.artifact = artifact;
    state.certified_graph = local;
    state.region = region;
    work.reduction_columns_reused += repair_work.columns_reused();
    work.reduction_columns_reduced += repair_work.columns_reduced();
    work.reduction_column_additions += repair_work.column_additions;
    events.push(repair_event(state, changed, repair_mode, work));
    Ok(())
}

fn repair_event(
    state: &ProgramAtomState,
    changed: &BTreeSet<EdgeKey>,
    repair_mode: crate::ReductionRepairMode,
    work: &mut ProgramWork,
) -> ProgramEvent {
    let kind = if repair_mode == crate::ReductionRepairMode::SuffixRepaired {
        work.atoms_repaired += 1;
        ProgramEventKind::ReductionSuffixRepaired
    } else {
        work.atoms_rebuilt += 1;
        ProgramEventKind::AtomRebuilt
    };
    ProgramEvent {
        kind,
        atom: Some(state.info_index),
        edge: state
            .edges
            .iter()
            .find(|edge| changed.contains(edge))
            .copied(),
        guard: None,
    }
}

fn preview_atom_state(
    state: &ProgramAtomState,
    updated: &SparseDistanceMatrix,
    changed: &BTreeSet<EdgeKey>,
    modulus: u32,
    work: &mut ProgramWork,
    events: &mut Vec<ProgramEvent>,
) -> Result<()> {
    work.atoms_touched += 1;
    work.guards_checked += state.region.guards().len();
    let local = local_matrix(&state.vertices, &state.edges, updated)?;
    if reusable_explanation(state, &local, modulus, events)?.is_some() {
        work.atoms_reused += 1;
    } else {
        work.atoms_rebuilt += 1;
        events.push(ProgramEvent {
            kind: ProgramEventKind::AtomRebuilt,
            atom: Some(state.info_index),
            edge: state
                .edges
                .iter()
                .find(|edge| changed.contains(edge))
                .copied(),
            guard: None,
        });
    }
    Ok(())
}

fn compile_atoms(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    limits: CertificateLimits,
    blocks: &[ProgramBlock],
) -> Result<(Vec<ProgramAtomInfo>, Vec<ProgramAtomState>)> {
    let atoms = atom_infos(input, blocks);
    let topology: Vec<_> = input.edges().map(|(u, v, _)| EdgeKey::new(u, v)).collect();
    let mut states = Vec::new();
    for atom in &atoms {
        if !atom.cyclic {
            continue;
        }
        let local = local_matrix(&atom.vertices, &atom.edges, input)?;
        let (artifact, _) = AtlasArtifact::compile(&local, &atom_params(params), limits)
            .map_err(|error| Error::InvalidInput(error.to_string()))?;
        let region = artifact
            .reduction_certificate()
            .compile_region(&local, limits)?;
        states.push(ProgramAtomState {
            info_index: atom.id,
            vertices: atom.vertices.clone(),
            edges: atom.edges.clone(),
            edge_positions: atom
                .edges
                .iter()
                .map(|edge| {
                    topology
                        .binary_search(edge)
                        .expect("atom edge is in the program topology")
                })
                .collect(),
            explained: artifact.explained().clone(),
            artifact,
            certified_graph: local,
            region,
        });
    }
    Ok((atoms, states))
}

pub(crate) fn atom_infos(
    input: &SparseDistanceMatrix,
    blocks: &[ProgramBlock],
) -> Vec<ProgramAtomInfo> {
    let mut counts = vec![0usize; input.len()];
    for block in blocks {
        for &vertex in &block.vertices {
            counts[vertex] += 1;
        }
    }
    let atoms: Vec<_> = blocks
        .iter()
        .enumerate()
        .map(|(id, block)| ProgramAtomInfo {
            id,
            vertices: block.vertices.clone(),
            edges: block
                .edges
                .iter()
                .map(|&[u, v]| EdgeKey::new(u, v))
                .collect(),
            separator_vertices: block
                .vertices
                .iter()
                .copied()
                .filter(|&vertex| counts[vertex] > 1)
                .collect(),
            cyclic: block.edges.len() >= block.vertices.len(),
        })
        .collect();
    atoms
}

pub(crate) fn atom_params(params: &RipsParams) -> RipsParams {
    let mut atom = RipsParams::new(1).with_modulus(params.modulus);
    atom.threshold = params.threshold;
    atom
}

pub(crate) fn local_matrix(
    vertices: &[usize],
    edges: &[EdgeKey],
    input: &SparseDistanceMatrix,
) -> Result<SparseDistanceMatrix> {
    let triplets: Vec<_> = edges
        .iter()
        .map(|edge| {
            let u = vertices
                .binary_search(&edge.u)
                .expect("atom contains its edge endpoint");
            let v = vertices
                .binary_search(&edge.v)
                .expect("atom contains its edge endpoint");
            (u, v, input.get(edge.u, edge.v))
        })
        .collect();
    SparseDistanceMatrix::from_triplets(vertices.len(), &triplets)
}

fn program_summary(
    decomposition: ProgramDecompositionSummary,
    atoms: &[ProgramAtomInfo],
    states: &[ProgramAtomState],
) -> ProgramSummary {
    debug_assert!(decomposition.articulation.blocks <= atoms.len());
    ProgramSummary {
        atoms: atoms.len(),
        cyclic_atoms: atoms.iter().filter(|atom| atom.cyclic).count(),
        articulation_vertices: decomposition.articulation_vertices,
        zero_simplex_separators: decomposition.zero_simplex_separators,
        widest_separator: decomposition.widest_separator,
        separator_candidates_checked: decomposition.separator_candidates_checked,
        separator_search_complete: decomposition.separator_search_complete,
        largest_cyclic_atom_edges: atoms
            .iter()
            .filter(|atom| atom.cyclic)
            .map(|atom| atom.edges.len())
            .max()
            .unwrap_or(0),
        guards: states.iter().map(|state| state.region.guards().len()).sum(),
    }
}

fn separator_stats(atoms: &[ProgramAtomInfo]) -> (usize, usize, usize) {
    let mut separators = BTreeSet::new();
    let mut articulations = BTreeSet::new();
    for (position, left) in atoms.iter().enumerate() {
        for right in &atoms[position + 1..] {
            let intersection: Vec<_> = left
                .vertices
                .iter()
                .copied()
                .filter(|vertex| right.vertices.binary_search(vertex).is_ok())
                .collect();
            if intersection.len() > 1 {
                separators.insert(intersection);
            } else if let Some(&vertex) = intersection.first() {
                articulations.insert(vertex);
            }
        }
    }
    let widest = separators.iter().map(Vec::len).max().unwrap_or(1);
    (articulations.len(), separators.len(), widest)
}

fn wider_separator_edges(atoms: &[ProgramAtomInfo]) -> Vec<EdgeKey> {
    let mut edges = BTreeSet::new();
    for (position, left) in atoms.iter().enumerate() {
        for right in &atoms[position + 1..] {
            let intersection: Vec<_> = left
                .vertices
                .iter()
                .copied()
                .filter(|vertex| right.vertices.binary_search(vertex).is_ok())
                .collect();
            if intersection.len() < 2 {
                continue;
            }
            for (position, &u) in intersection.iter().enumerate() {
                for &v in &intersection[position + 1..] {
                    edges.insert(EdgeKey::new(u, v));
                }
            }
        }
    }
    edges.into_iter().collect()
}

pub(crate) fn compose_result(
    input: &SparseDistanceMatrix,
    params: &RipsParams,
    states: &[ProgramAtomState],
) -> Result<ExplainedDiagram> {
    let mut seeds = Vec::new();
    let terminal = terminal_level(input, params.threshold);
    for state in states {
        for space in &state.explained.spaces {
            let interval = space.interval;
            let scale = if interval.death.is_finite() {
                previous_float(interval.death)
            } else {
                terminal
            };
            let cocycles = space
                .basis
                .iter()
                .map(|class| Cocycle {
                    modulus: class.cocycle.modulus,
                    scale,
                    terms: class
                        .cocycle
                        .terms
                        .iter()
                        .map(|term| CocycleTerm {
                            u: state.vertices[term.u],
                            v: state.vertices[term.v],
                            coefficient: term.coefficient,
                        })
                        .collect(),
                })
                .collect();
            let critical_pairs = space
                .critical_pairs
                .iter()
                .map(|pair| map_critical_pair(pair, &state.vertices))
                .collect();
            seeds.push(SpaceSeed {
                interval,
                cocycles,
                critical_pairs,
            });
        }
    }
    let spaces = merge_spaces(input, params.modulus, seeds)?;
    let (mut diagram, _) = h0_diagram(input, params.threshold);
    for space in &spaces {
        diagram
            .bars
            .extend(std::iter::repeat_n(space.interval, space.basis.len()));
    }
    diagram.canonicalize();
    Ok(ExplainedDiagram { diagram, spaces })
}

#[derive(Debug)]
struct SpaceSeed {
    interval: Bar,
    cocycles: Vec<Cocycle>,
    critical_pairs: Vec<CriticalPair>,
}

fn merge_spaces(
    input: &SparseDistanceMatrix,
    modulus: u32,
    seeds: Vec<SpaceSeed>,
) -> Result<Vec<PersistentClassSpace>> {
    let mut groups: BTreeMap<(u64, u64), SpaceSeed> = BTreeMap::new();
    for seed in seeds {
        let key = (seed.interval.birth.to_bits(), seed.interval.death.to_bits());
        let group = groups.entry(key).or_insert_with(|| SpaceSeed {
            interval: seed.interval,
            cocycles: Vec::new(),
            critical_pairs: Vec::new(),
        });
        group.cocycles.extend(seed.cocycles);
        group.critical_pairs.extend(seed.critical_pairs);
    }
    let mut spaces = Vec::with_capacity(groups.len());
    for mut seed in groups.into_values() {
        let cocycles = canonical_space_basis(input, modulus, &seed.cocycles)?;
        if cocycles.len() != seed.critical_pairs.len() {
            return Err(Error::InvalidInput(format!(
                "composed class-space rank {} differs from critical-pair count {}",
                cocycles.len(),
                seed.critical_pairs.len()
            )));
        }
        for cocycle in &cocycles {
            validate_h1_cocycle(input, cocycle)?;
        }
        seed.critical_pairs.sort_by(critical_pair_order);
        let id = group_id(seed.interval, modulus, &cocycles);
        let basis = cocycles
            .into_iter()
            .enumerate()
            .map(|(basis_index, cocycle)| PersistentClass {
                id: basis_class_id(id, basis_index, &cocycle),
                group_id: id,
                basis_index,
                interval: seed.interval,
                cocycle,
            })
            .collect();
        spaces.push(PersistentClassSpace {
            id,
            interval: seed.interval,
            basis,
            critical_pairs: seed.critical_pairs,
        });
    }
    spaces.sort_by(|a, b| {
        a.interval
            .birth
            .total_cmp(&b.interval.birth)
            .then(a.interval.death.total_cmp(&b.interval.death))
            .then(a.id.cmp(&b.id))
    });
    Ok(spaces)
}

fn reweight_explained(
    input: &SparseDistanceMatrix,
    previous: &ExplainedDiagram,
    evaluation: &crate::CertifiedRegionEvaluation,
    modulus: u32,
) -> Result<Option<ExplainedDiagram>> {
    if evaluation.h1_critical_pairs().len() != previous.class_count() {
        return Ok(None);
    }
    let records: BTreeMap<_, _> = evaluation
        .h1_critical_pairs()
        .iter()
        .map(|(bar, pair)| (critical_pair_key(pair), (*bar, pair.clone())))
        .collect();
    let terminal = terminal_level(input, None);
    let mut seeds = Vec::new();
    for space in &previous.spaces {
        let mut interval = None;
        let mut pairs = Vec::with_capacity(space.critical_pairs.len());
        for pair in &space.critical_pairs {
            let Some((bar, updated_pair)) = records.get(&critical_pair_key(pair)) else {
                return Ok(None);
            };
            if interval.is_some_and(|interval: Bar| !bar_bits_equal(interval, *bar)) {
                return Ok(None);
            }
            interval = Some(*bar);
            pairs.push(updated_pair.clone());
        }
        let Some(interval) = interval else {
            return Ok(None);
        };
        let scale = if interval.death.is_finite() {
            previous_float(interval.death)
        } else {
            terminal
        };
        let cocycles: Vec<_> = space
            .basis
            .iter()
            .map(|class| Cocycle {
                modulus,
                scale,
                terms: class.cocycle.terms.clone(),
            })
            .collect();
        if cocycles
            .iter()
            .any(|cocycle| validate_h1_cocycle(input, cocycle).is_err())
        {
            return Ok(None);
        }
        seeds.push(SpaceSeed {
            interval,
            cocycles,
            critical_pairs: pairs,
        });
    }
    let spaces = merge_spaces(input, modulus, seeds)?;
    Ok(Some(ExplainedDiagram {
        diagram: evaluation.diagram().clone(),
        spaces,
    }))
}

pub(crate) fn class_continuation(
    old: &[PersistentClassSpace],
    new: &[PersistentClassSpace],
) -> Vec<ClassContinuation> {
    let old_terms = basis_terms(old);
    let new_terms = basis_terms(new);
    let (adjacency, transports) = continuation_graph(old.len(), new.len(), &old_terms, &new_terms);
    let mut seen = vec![false; adjacency.len()];
    let mut output = Vec::new();
    for start in 0..adjacency.len() {
        if seen[start] || adjacency[start].is_empty() {
            continue;
        }
        let (old_nodes, new_nodes) =
            continuation_component(start, old.len(), &adjacency, &mut seen);
        output.push(component_continuation(
            old,
            new,
            &old_nodes,
            &new_nodes,
            &transports,
        ));
    }
    output.extend(unmatched_continuations(old, new, &adjacency));
    output.sort_by(|a, b| {
        a.old_spaces
            .cmp(&b.old_spaces)
            .then(a.new_spaces.cmp(&b.new_spaces))
    });
    output
}

type BasisTerms = BTreeMap<Vec<CocycleTerm>, Vec<(usize, BasisClassId)>>;
type ContinuationTransports = BTreeMap<(usize, usize), Vec<BasisTransport>>;

fn basis_terms(spaces: &[PersistentClassSpace]) -> BasisTerms {
    let mut terms = BasisTerms::new();
    for (space, item) in spaces.iter().enumerate() {
        for class in &item.basis {
            terms
                .entry(class.cocycle.terms.clone())
                .or_default()
                .push((space, class.id));
        }
    }
    terms
}

fn continuation_graph(
    old_count: usize,
    new_count: usize,
    old_terms: &BasisTerms,
    new_terms: &BasisTerms,
) -> (Vec<BTreeSet<usize>>, ContinuationTransports) {
    let mut adjacency = vec![BTreeSet::new(); old_count + new_count];
    let mut transports = ContinuationTransports::new();
    for (terms, old_basis) in old_terms {
        let Some(new_basis) = new_terms.get(terms) else {
            continue;
        };
        connect_matching_basis(
            old_count,
            old_basis,
            new_basis,
            &mut adjacency,
            &mut transports,
        );
    }
    (adjacency, transports)
}

fn connect_matching_basis(
    old_count: usize,
    old_basis: &[(usize, BasisClassId)],
    new_basis: &[(usize, BasisClassId)],
    adjacency: &mut [BTreeSet<usize>],
    transports: &mut ContinuationTransports,
) {
    for &(old_space, old_id) in old_basis {
        for &(new_space, new_id) in new_basis {
            let new_node = old_count + new_space;
            adjacency[old_space].insert(new_node);
            adjacency[new_node].insert(old_space);
            transports
                .entry((old_space, new_space))
                .or_default()
                .push(BasisTransport {
                    old: old_id,
                    new: new_id,
                    coefficient: 1,
                });
        }
    }
}

fn continuation_component(
    start: usize,
    old_count: usize,
    adjacency: &[BTreeSet<usize>],
    seen: &mut [bool],
) -> (Vec<usize>, Vec<usize>) {
    let mut queue = VecDeque::from([start]);
    seen[start] = true;
    let mut old_nodes = Vec::new();
    let mut new_nodes = Vec::new();
    while let Some(node) = queue.pop_front() {
        if node < old_count {
            old_nodes.push(node);
        } else {
            new_nodes.push(node - old_count);
        }
        enqueue_unseen(&adjacency[node], seen, &mut queue);
    }
    old_nodes.sort_unstable();
    new_nodes.sort_unstable();
    (old_nodes, new_nodes)
}

fn enqueue_unseen(adjacency: &BTreeSet<usize>, seen: &mut [bool], queue: &mut VecDeque<usize>) {
    for &next in adjacency {
        if !seen[next] {
            seen[next] = true;
            queue.push_back(next);
        }
    }
}

fn component_continuation(
    old: &[PersistentClassSpace],
    new: &[PersistentClassSpace],
    old_nodes: &[usize],
    new_nodes: &[usize],
    transports: &ContinuationTransports,
) -> ClassContinuation {
    let mut transport = component_transports(old_nodes, new_nodes, transports);
    transport.sort_by_key(|term| (term.old, term.new));
    transport.dedup();
    let old_rank: usize = old_nodes.iter().map(|&index| old[index].basis.len()).sum();
    let new_rank: usize = new_nodes.iter().map(|&index| new[index].basis.len()).sum();
    let complete = transport.len() == old_rank && transport.len() == new_rank;
    ClassContinuation {
        kind: continuation_kind(old_nodes.len(), new_nodes.len(), complete),
        old_spaces: old_nodes.iter().map(|&index| old[index].id).collect(),
        new_spaces: new_nodes.iter().map(|&index| new[index].id).collect(),
        transport,
    }
}

fn component_transports(
    old_nodes: &[usize],
    new_nodes: &[usize],
    transports: &ContinuationTransports,
) -> Vec<BasisTransport> {
    let mut output = Vec::new();
    for &old_space in old_nodes {
        for &new_space in new_nodes {
            if let Some(terms) = transports.get(&(old_space, new_space)) {
                output.extend(terms.iter().copied());
            }
        }
    }
    output
}

fn continuation_kind(old_count: usize, new_count: usize, complete: bool) -> ContinuationKind {
    match (old_count, new_count, complete) {
        (1, 1, true) => ContinuationKind::Isomorphism,
        (1, many, true) if many > 1 => ContinuationKind::Split,
        (many, 1, true) if many > 1 => ContinuationKind::Merge,
        (many_old, many_new, true) if many_old > 1 && many_new > 1 => ContinuationKind::Mixing,
        _ => ContinuationKind::Ambiguous,
    }
}

fn unmatched_continuations(
    old: &[PersistentClassSpace],
    new: &[PersistentClassSpace],
    adjacency: &[BTreeSet<usize>],
) -> Vec<ClassContinuation> {
    let deaths = old
        .iter()
        .enumerate()
        .filter(|(index, _)| adjacency[*index].is_empty())
        .map(|(_, space)| unmatched_continuation(ContinuationKind::Death, space.id));
    let births = new
        .iter()
        .enumerate()
        .filter(|(index, _)| adjacency[old.len() + *index].is_empty())
        .map(|(_, space)| unmatched_continuation(ContinuationKind::Birth, space.id));
    deaths.chain(births).collect()
}

fn unmatched_continuation(kind: ContinuationKind, space: IntervalGroupId) -> ClassContinuation {
    let (old_spaces, new_spaces) = match kind {
        ContinuationKind::Death => (vec![space], Vec::new()),
        ContinuationKind::Birth => (Vec::new(), vec![space]),
        _ => unreachable!("only births and deaths are unmatched"),
    };
    ClassContinuation {
        kind,
        old_spaces,
        new_spaces,
        transport: Vec::new(),
    }
}

pub(crate) fn program_topology_events(
    program: &PersistenceProgram,
    updated: &SparseDistanceMatrix,
) -> Vec<ProgramEvent> {
    if updated.len() != program.graph.len() {
        return vec![ProgramEvent {
            kind: ProgramEventKind::VertexSetChanged,
            atom: None,
            edge: None,
            guard: None,
        }];
    }
    let topology: Vec<_> = updated
        .edges()
        .map(|(u, v, _)| EdgeKey::new(u, v))
        .collect();
    if topology != program.topology {
        let edge = program
            .topology
            .iter()
            .find(|edge| topology.binary_search(edge).is_err())
            .or_else(|| {
                topology
                    .iter()
                    .find(|edge| program.topology.binary_search(edge).is_err())
            })
            .copied();
        return vec![ProgramEvent {
            kind: ProgramEventKind::EdgeSetChanged,
            atom: None,
            edge,
            guard: None,
        }];
    }
    let threshold = program.params.threshold.unwrap_or(f64::INFINITY);
    let mut events: Vec<_> = program
        .topology
        .iter()
        .enumerate()
        .filter_map(|(index, &edge)| {
            ((updated.get(edge.u, edge.v) <= threshold) != program.active[index]).then_some(
                ProgramEvent {
                    kind: ProgramEventKind::ThresholdCrossing,
                    atom: None,
                    edge: Some(edge),
                    guard: None,
                },
            )
        })
        .collect();
    if events.is_empty() {
        events.extend(program.separator_edges.iter().filter_map(|&edge| {
            (updated.get(edge.u, edge.v).to_bits() != 0).then_some(ProgramEvent {
                kind: ProgramEventKind::SeparatorContractChanged,
                atom: None,
                edge: Some(edge),
                guard: None,
            })
        }));
    }
    events
}

fn check_program_topology(
    program: &PersistenceProgram,
    updated: &SparseDistanceMatrix,
) -> Result<Vec<f64>> {
    if updated.len() != program.graph.len() {
        return Err(Error::InvalidInput(
            "persistence program topology ended at VertexSetChanged".into(),
        ));
    }
    if updated.num_edges() != program.topology.len() {
        return Err(Error::InvalidInput(
            "persistence program topology ended at EdgeSetChanged".into(),
        ));
    }
    let threshold = program.params.threshold.unwrap_or(f64::INFINITY);
    let mut values = Vec::with_capacity(program.topology.len());
    for ((u, v, weight), (&edge, &active)) in updated
        .edges()
        .zip(program.topology.iter().zip(&program.active))
    {
        if edge != EdgeKey::new(u, v) {
            return Err(Error::InvalidInput(
                "persistence program topology ended at EdgeSetChanged".into(),
            ));
        }
        if (weight <= threshold) != active {
            return Err(Error::InvalidInput(
                "persistence program topology ended at ThresholdCrossing".into(),
            ));
        }
        values.push(weight);
    }
    if program
        .separator_edges
        .iter()
        .any(|edge| updated.get(edge.u, edge.v).to_bits() != 0)
    {
        return Err(Error::InvalidInput(
            "persistence program topology ended at SeparatorContractChanged".into(),
        ));
    }
    Ok(values)
}

fn h0_diagram(input: &SparseDistanceMatrix, threshold: Option<f64>) -> (Diagram, usize) {
    let (deaths, components, scanned) = h0_provenance(input, threshold);
    let mut diagram = Diagram::default();
    for edge in deaths {
        let death = input.get(edge.u, edge.v);
        if death > 0.0 {
            diagram.bars.push(Bar {
                dim: 0,
                birth: 0.0,
                death,
            });
        }
    }
    diagram.bars.extend((0..components).map(|_| Bar {
        dim: 0,
        birth: 0.0,
        death: f64::INFINITY,
    }));
    diagram.canonicalize();
    (diagram, scanned)
}

fn h0_provenance(
    input: &SparseDistanceMatrix,
    threshold: Option<f64>,
) -> (Vec<EdgeKey>, usize, usize) {
    let threshold = threshold.unwrap_or(f64::INFINITY);
    let mut edges: Vec<_> = input
        .edges()
        .filter(|&(_, _, value)| value <= threshold)
        .collect();
    edges.sort_by(|a, b| {
        a.2.total_cmp(&b.2)
            .then_with(|| edge_rank([b.0, b.1]).cmp(&edge_rank([a.0, a.1])))
    });
    let scanned = edges.len();
    let mut parent: Vec<_> = (0..input.len()).collect();
    let mut rank = vec![0u8; input.len()];
    let mut deaths = Vec::new();
    for (u, v, _) in edges {
        let left = dsu_find(&mut parent, u);
        let right = dsu_find(&mut parent, v);
        if left == right {
            continue;
        }
        dsu_link(&mut parent, &mut rank, left, right);
        deaths.push(EdgeKey::new(u, v));
    }
    let components = (0..input.len())
        .filter(|&vertex| dsu_find(&mut parent, vertex) == vertex)
        .count();
    (deaths, components, scanned)
}

fn dsu_find(parent: &mut [usize], mut vertex: usize) -> usize {
    let mut root = vertex;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[vertex] != root {
        let next = parent[vertex];
        parent[vertex] = root;
        vertex = next;
    }
    root
}

fn dsu_link(parent: &mut [usize], rank: &mut [u8], left: usize, right: usize) {
    if rank[left] < rank[right] {
        parent[left] = right;
    } else {
        parent[right] = left;
        if rank[left] == rank[right] {
            rank[left] += 1;
        }
    }
}

fn map_critical_pair(pair: &CriticalPair, vertices: &[usize]) -> CriticalPair {
    CriticalPair {
        birth: crate::CriticalSimplex {
            vertices: pair.birth.vertices.iter().map(|&v| vertices[v]).collect(),
            value: pair.birth.value,
        },
        death: pair.death.as_ref().map(|death| crate::CriticalSimplex {
            vertices: death.vertices.iter().map(|&v| vertices[v]).collect(),
            value: death.value,
        }),
    }
}

fn critical_pair_key(pair: &CriticalPair) -> (Vec<usize>, Option<Vec<usize>>) {
    (
        pair.birth.vertices.clone(),
        pair.death.as_ref().map(|simplex| simplex.vertices.clone()),
    )
}

fn critical_pair_order(a: &CriticalPair, b: &CriticalPair) -> std::cmp::Ordering {
    critical_pair_key(a).cmp(&critical_pair_key(b))
}

fn simplex_edge(simplex: &crate::FiltrationSimplex) -> Option<EdgeKey> {
    match simplex.vertices() {
        &[u, v] => Some(EdgeKey::new(u, v)),
        _ => None,
    }
}

fn terminal_level(input: &SparseDistanceMatrix, threshold: Option<f64>) -> f64 {
    threshold.unwrap_or_else(|| {
        input
            .edges()
            .map(|(_, _, value)| value)
            .fold(0.0f64, f64::max)
    })
}

fn previous_float(value: f64) -> f64 {
    debug_assert!(value.is_finite() && value > 0.0);
    f64::from_bits(value.to_bits() - 1)
}

fn edge_rank([u, v]: [usize; 2]) -> u128 {
    v as u128 * (v.saturating_sub(1)) as u128 / 2 + u as u128
}

fn bar_bits_equal(a: Bar, b: Bar) -> bool {
    a.dim == b.dim
        && a.birth.to_bits() == b.birth.to_bits()
        && a.death.to_bits() == b.death.to_bits()
}

fn diagram_bits_equal(a: &Diagram, b: &Diagram) -> bool {
    a.bars.len() == b.bars.len()
        && a.bars.iter().zip(&b.bars).all(|(a, b)| {
            a.dim == b.dim
                && a.birth.to_bits() == b.birth.to_bits()
                && a.death.to_bits() == b.death.to_bits()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_squares() -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, 4.5),
            ],
        )
        .unwrap()
    }

    fn two_cycles_on_zero_edge(separator_weight: f64) -> SparseDistanceMatrix {
        SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, separator_weight),
                (0, 2, 1.0),
                (2, 3, 2.0),
                (1, 3, 3.0),
                (0, 4, 1.5),
                (4, 5, 2.5),
                (1, 5, 3.5),
            ],
        )
        .unwrap()
    }

    #[test]
    fn program_composes_articulation_blocks_exactly() {
        let graph = two_squares();
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let program =
                PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
            assert_eq!(program.summary().cyclic_atoms, 2);
            assert_eq!(program.summary().articulation_vertices, 1);
            let expected = rips_persistence_sparse(&graph, &params).unwrap();
            assert!(diagram_bits_equal(&program.result().diagram, &expected));
            assert_eq!(program.result().class_count(), 2);
        }
    }

    #[test]
    fn program_composes_across_a_zero_filtration_edge_separator() {
        let graph = two_cycles_on_zero_edge(0.0);
        for modulus in [2, 3, 5] {
            let params = RipsParams::new(1).with_modulus(modulus);
            let program =
                PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
            assert_eq!(program.summary().cyclic_atoms, 2);
            assert_eq!(program.summary().articulation_vertices, 0);
            assert_eq!(program.summary().zero_simplex_separators, 1);
            assert_eq!(program.summary().widest_separator, 2);
            let expected = rips_persistence_sparse(&graph, &params).unwrap();
            assert!(diagram_bits_equal(&program.result().diagram, &expected));
            assert_eq!(program.result().class_count(), 2);
        }

        let later_separator = two_cycles_on_zero_edge(0.25);
        let unsplit = PersistenceProgram::compile(
            &later_separator,
            &RipsParams::new(1),
            CertificateLimits::default(),
        )
        .unwrap();
        assert_eq!(unsplit.summary().cyclic_atoms, 1);
        assert_eq!(unsplit.summary().zero_simplex_separators, 0);

        let mut changing =
            PersistenceProgram::compile(&graph, &RipsParams::new(1), CertificateLimits::default())
                .unwrap();
        let update = changing.advance(&later_separator).unwrap();
        assert_eq!(update.mode, ProgramUpdateMode::Recompiled);
        assert!(
            update
                .events
                .iter()
                .any(|event| event.kind == ProgramEventKind::SeparatorContractChanged)
        );
    }

    #[test]
    fn update_rebuilds_only_the_touched_atom() {
        let graph = two_squares();
        let params = RipsParams::new(1).with_modulus(3);
        let mut program =
            PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
        let updated = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 5.0),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, 4.5),
            ],
        )
        .unwrap();
        let step = program.advance(&updated).unwrap();
        assert_eq!(step.work.atoms_touched, 1);
        assert!(step.work.atoms_rebuilt <= 1);
        let expected = rips_persistence_sparse(&updated, &params).unwrap();
        assert!(diagram_bits_equal(&step.result.diagram, &expected));
    }

    #[test]
    fn ordered_batches_are_atomic_and_checkpoints_restore_exact_state() {
        let initial = two_squares();
        let first = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.1),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, 4.5),
            ],
        )
        .unwrap();
        let second = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.2),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.5),
                (4, 5, 2.5),
                (5, 6, 3.5),
                (3, 6, 4.5),
            ],
        )
        .unwrap();
        let invalid = SparseDistanceMatrix::from_triplets(6, &[]).unwrap();
        let params = RipsParams::new(1).with_modulus(3);
        let mut program =
            PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
        let checkpoint = program.checkpoint();

        // Force the second, topology-changing step through the compile-time
        // parameter gate after the first step has already succeeded.
        program.params.max_dim = 2;
        assert!(program.advance_batch(&[first.clone(), invalid]).is_err());
        assert!(diagram_bits_equal(
            &program.result().diagram,
            &checkpoint.program().result().diagram
        ));
        assert_eq!(
            program.current_graph().edges().collect::<Vec<_>>(),
            initial.edges().collect::<Vec<_>>()
        );

        program.params.max_dim = 1;
        let updates = program
            .advance_batch(&[first.clone(), second.clone()])
            .unwrap();
        assert_eq!(updates.len(), 2);
        let expected = rips_persistence_sparse(&second, &params).unwrap();
        assert!(diagram_bits_equal(&program.result().diagram, &expected));
        program.restore(&checkpoint);
        assert_eq!(
            program.current_graph().edges().collect::<Vec<_>>(),
            initial.edges().collect::<Vec<_>>()
        );
    }

    #[test]
    fn parallel_branches_preserve_order_and_leave_the_base_unchanged() {
        let initial = two_squares();
        let alternatives: Vec<_> = [1.1, 1.2, 1.3, 1.4]
            .into_iter()
            .map(|weight| {
                SparseDistanceMatrix::from_triplets(
                    7,
                    &[
                        (0, 1, weight),
                        (1, 2, 2.0),
                        (2, 3, 3.0),
                        (0, 3, 4.0),
                        (3, 4, 1.5),
                        (4, 5, 2.5),
                        (5, 6, 3.5),
                        (3, 6, 4.5),
                    ],
                )
                .unwrap()
            })
            .collect();
        let mut params = RipsParams::new(1).with_modulus(5);
        params.threads = 4;
        let program =
            PersistenceProgram::compile(&initial, &params, CertificateLimits::default()).unwrap();
        let branches = program.branch(&alternatives).unwrap();
        assert_eq!(
            branches
                .iter()
                .map(|branch| branch.index)
                .collect::<Vec<_>>(),
            vec![0, 1, 2, 3]
        );
        for (branch, graph) in branches.iter().zip(&alternatives) {
            let expected = rips_persistence_sparse(graph, &params).unwrap();
            assert!(diagram_bits_equal(&branch.update.result.diagram, &expected));
            assert!(diagram_bits_equal(
                &branch.program().result().diagram,
                &expected
            ));
        }
        assert_eq!(
            program.current_graph().edges().collect::<Vec<_>>(),
            initial.edges().collect::<Vec<_>>()
        );
    }

    #[test]
    fn unchanged_basis_vectors_receive_exact_continuation() {
        let graph = two_squares();
        let params = RipsParams::new(1);
        let mut program =
            PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
        let updated = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.01),
                (1, 2, 2.01),
                (2, 3, 3.01),
                (0, 3, 4.01),
                (3, 4, 1.51),
                (4, 5, 2.51),
                (5, 6, 3.51),
                (3, 6, 4.51),
            ],
        )
        .unwrap();
        let step = program.advance(&updated).unwrap();
        assert!(
            step.continuation
                .iter()
                .all(|record| record.kind == ContinuationKind::Isomorphism)
        );
        assert_eq!(
            step.continuation
                .iter()
                .map(|record| record.transport.len())
                .sum::<usize>(),
            2
        );
        assert!(!step.correspondence.is_empty());
        assert!(
            step.correspondence
                .iter()
                .all(ClassCorrespondence::is_isomorphism)
        );
        assert_eq!(
            step.correspondence
                .iter()
                .map(|record| record.relation_rank)
                .sum::<usize>(),
            2
        );

        let mut state_only =
            PersistenceProgram::compile(&graph, &params, CertificateLimits::default()).unwrap();
        let untracked = state_only
            .advance_with(&updated, CorrespondenceMode::Omit)
            .unwrap();
        assert!(untracked.correspondence.is_empty());
        assert!(diagram_bits_equal(
            &untracked.result.diagram,
            &step.result.diagram
        ));
        assert_eq!(untracked.result.spaces, step.result.spaces);
    }

    #[test]
    fn equal_interval_space_splits_and_merges_without_false_ids() {
        let equal = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.0),
                (4, 5, 2.0),
                (5, 6, 3.0),
                (3, 6, 4.0),
            ],
        )
        .unwrap();
        let split = SparseDistanceMatrix::from_triplets(
            7,
            &[
                (0, 1, 1.0),
                (1, 2, 2.0),
                (2, 3, 3.0),
                (0, 3, 4.0),
                (3, 4, 1.0),
                (4, 5, 2.0),
                (5, 6, 3.0),
                (3, 6, 4.5),
            ],
        )
        .unwrap();
        let params = RipsParams::new(1).with_modulus(5);
        let mut program =
            PersistenceProgram::compile(&equal, &params, CertificateLimits::default()).unwrap();
        assert_eq!(program.result().spaces.len(), 1);
        assert_eq!(program.result().spaces[0].basis.len(), 2);
        let separated = program.advance(&split).unwrap();
        assert_eq!(separated.result.spaces.len(), 2);
        assert_eq!(separated.continuation.len(), 1);
        assert_eq!(separated.continuation[0].kind, ContinuationKind::Split);
        let merged = program.advance(&equal).unwrap();
        assert_eq!(merged.result.spaces.len(), 1);
        assert_eq!(merged.continuation.len(), 1);
        assert_eq!(merged.continuation[0].kind, ContinuationKind::Merge);
    }

    #[test]
    fn random_program_updates_match_monolithic_reduction() {
        let mut state = 0x735a_41ce_9b20_d68fu64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for case in 0..32 {
            let n = 5 + next() as usize % 7;
            let mut triplets = Vec::new();
            for u in 0..n {
                for v in u + 1..n {
                    if next() % 5 < 3 {
                        triplets.push((u, v, (1 + next() % 12) as f64));
                    }
                }
            }
            let graph = SparseDistanceMatrix::from_triplets(n, &triplets).unwrap();
            for modulus in [2, 3, 5] {
                let params = RipsParams::new(1).with_modulus(modulus);
                let mut program =
                    PersistenceProgram::compile(&graph, &params, CertificateLimits::default())
                        .unwrap_or_else(|error| panic!("case {case}, modulus {modulus}: {error}"));
                for step in 0..4 {
                    let updated_triplets: Vec<_> = triplets
                        .iter()
                        .map(|&(u, v, value)| {
                            let delta = (next() % 9) as f64 * 0.1 * (step + 1) as f64;
                            (u, v, value + delta)
                        })
                        .collect();
                    let updated =
                        SparseDistanceMatrix::from_triplets(n, &updated_triplets).unwrap();
                    let actual = program.advance(&updated).unwrap_or_else(|error| {
                        panic!("case {case}, modulus {modulus}, step {step}: {error}")
                    });
                    let expected = rips_persistence_sparse(&updated, &params).unwrap();
                    assert!(
                        diagram_bits_equal(&actual.result.diagram, &expected),
                        "case {case}, modulus {modulus}, step {step}"
                    );
                }
            }
        }
    }
}
