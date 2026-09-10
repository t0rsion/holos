use std::collections::BTreeSet;

use crate::{
    CertificateLimits, ClassCorrespondence, EdgeKey, Error, ExplainedDiagram, PersistentClassSpace,
    Result, RipsParams, SparseDistanceMatrix,
};

use super::composition::{compose_result, local_matrix, reweight_explained};
use super::continuation::class_continuation;
use super::model::{
    CorrespondenceMode, PersistenceProgram, ProgramAtomInfo, ProgramAtomState, ProgramBranch,
    ProgramCheckpoint, ProgramEvaluation, ProgramEvent, ProgramSummary, ProgramUpdate,
    ProgramUpdateMode, ProgramWork,
};
use super::topology::{check_program_topology, h0_diagram, h0_provenance, program_topology_events};

mod atom;

use atom::{preview_atom_state, update_atom_state};

impl PersistenceProgram {
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

    /// Capture the current state for later restore or branching.
    pub fn checkpoint(&self) -> ProgramCheckpoint {
        ProgramCheckpoint {
            program: self.clone(),
        }
    }

    /// Replace this program with a captured state.
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
        let (mut diagram, scanned) = h0_diagram(updated, self.params.threshold);
        work.h0_edges_scanned = scanned;
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
        let changed = changed_edges(self, updated);
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
            let Some(explained) = reweight_explained(
                &local,
                &state.explained,
                &evaluation,
                self.params.modulus,
                self.params.threshold,
            )?
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
                self.limits,
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
