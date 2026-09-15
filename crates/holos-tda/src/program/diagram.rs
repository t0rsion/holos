use crate::{Error, ExplainedDiagram, RegionViolationKind, Result, SparseDistanceMatrix};

use super::composition::local_matrix;
use super::model::{
    CorrespondenceMode, PersistenceProgram, ProgramAtomState, ProgramDiagramState,
    ProgramDiagramUpdate, ProgramDiagramUpdateMode, ProgramEvent, ProgramEventKind, ProgramWork,
};
use super::topology::{diagram_bits_equal, program_topology_events, simplex_edge};
use super::update::recompile_work;

impl PersistenceProgram {
    /// Move a compiled program into stateful diagram evaluation.
    pub fn into_diagram_state(self) -> ProgramDiagramState {
        let graph = self.graph.clone();
        let diagram = self.result.diagram.clone();
        ProgramDiagramState {
            program: self,
            graph,
            diagram,
            dirty: false,
        }
    }
}

impl ProgramDiagramState {
    /// Copy a compiled program into stateful diagram evaluation.
    pub fn from_program(program: &PersistenceProgram) -> Self {
        program.clone().into_diagram_state()
    }

    /// Current exact persistence diagram.
    pub fn diagram(&self) -> &crate::Diagram {
        &self.diagram
    }

    /// Advance the state and return only the exact persistence diagram.
    pub fn advance(&mut self, updated: &SparseDistanceMatrix) -> Result<ProgramDiagramUpdate> {
        let topology_events = program_topology_events(&self.program, updated);
        if !topology_events.is_empty() {
            return self.recompile(updated, topology_events);
        }
        if graph_bits_equal(&self.graph, updated) {
            return Ok(ProgramDiagramUpdate {
                diagram: self.diagram.clone(),
                mode: ProgramDiagramUpdateMode::Reused,
                events: Vec::new(),
                work: ProgramWork {
                    edges_checked: self.program.topology.len(),
                    ..ProgramWork::default()
                },
            });
        }

        match self.program.evaluate_diagram(updated) {
            Ok(evaluation) => {
                let diagram = evaluation.diagram;
                self.graph = updated.clone();
                self.diagram = diagram.clone();
                self.dirty = true;
                Ok(ProgramDiagramUpdate {
                    diagram,
                    mode: ProgramDiagramUpdateMode::Reused,
                    events: Vec::new(),
                    work: evaluation.work,
                })
            }
            Err(_) => {
                let events = region_violation_events(&self.program, updated)?;
                self.recompile(updated, events)
            }
        }
    }

    /// Materialize the current canonical class spaces and critical pairs.
    pub fn materialize(&mut self) -> Result<&ExplainedDiagram> {
        if !self.dirty {
            return Ok(self.program.result());
        }

        let mut candidate = self.program.clone();
        let update = match candidate.advance_with(&self.graph, CorrespondenceMode::Omit) {
            Ok(update) => update,
            Err(_) => return self.recompile_for_materialization(),
        };
        if !diagram_bits_equal(&update.result.diagram, &self.diagram) {
            return self.recompile_for_materialization();
        }

        self.program = candidate;
        self.dirty = false;
        Ok(self.program.result())
    }

    /// Materialize and consume this state as a normal persistence program.
    ///
    /// This method consumes the state even when materialization returns an
    /// error. Call [`ProgramDiagramState::materialize`] when the state must
    /// remain available after a failed operation.
    pub fn into_program(mut self) -> Result<PersistenceProgram> {
        self.materialize()?;
        Ok(self.program)
    }

    fn recompile(
        &mut self,
        updated: &SparseDistanceMatrix,
        events: Vec<ProgramEvent>,
    ) -> Result<ProgramDiagramUpdate> {
        let replacement =
            PersistenceProgram::compile(updated, &self.program.params, self.program.limits)?;
        let work = recompile_work(self.program.topology.len(), &replacement);
        let diagram = replacement.result().diagram.clone();
        self.program = replacement;
        self.graph = updated.clone();
        self.diagram = diagram.clone();
        self.dirty = false;
        Ok(ProgramDiagramUpdate {
            diagram,
            mode: ProgramDiagramUpdateMode::Recompiled,
            events,
            work,
        })
    }

    fn recompile_for_materialization(&mut self) -> Result<&ExplainedDiagram> {
        let graph = self.graph.clone();
        let params = self.program.params.clone();
        let replacement = PersistenceProgram::compile(&graph, &params, self.program.limits)?;
        let diagram = &replacement.result().diagram;
        if !diagram_bits_equal(diagram, &self.diagram) {
            return Err(Error::InvalidInput(
                "diagram materialization fallback differs from the accepted diagram".into(),
            ));
        }
        self.program = replacement;
        self.dirty = false;
        Ok(self.program.result())
    }
}

fn graph_bits_equal(left: &SparseDistanceMatrix, right: &SparseDistanceMatrix) -> bool {
    left.len() == right.len()
        && left.num_edges() == right.num_edges()
        && left
            .edges()
            .zip(right.edges())
            .all(|((lu, lv, lw), (ru, rv, rw))| {
                lu == ru && lv == rv && lw.to_bits() == rw.to_bits()
            })
}

fn region_violation_events(
    program: &PersistenceProgram,
    updated: &SparseDistanceMatrix,
) -> Result<Vec<ProgramEvent>> {
    let mut events = Vec::new();
    for state in program.states() {
        let local = local_matrix(&state.vertices, &state.edges, updated)?;
        events.extend(
            state
                .region
                .violations(&local)
                .iter()
                .map(|violation| violation_event(state, violation)),
        );
    }
    Ok(events)
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
