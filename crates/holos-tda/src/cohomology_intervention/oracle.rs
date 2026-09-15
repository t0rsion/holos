use crate::monotone_search::SearchStatus;
use crate::{
    CohomologyClassId, CohomologyLimits, CohomologySpace, Error, KineticEdgeKey, Result,
    SparseDistanceMatrix, cohomology_restriction, cohomology_space,
};

use super::model::{
    CohomologyInterventionCandidate, CohomologyInterventionScenario, CohomologyInterventionStatus,
};

pub(super) struct TopologyOracle<'a> {
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    scenarios: &'a [CohomologyInterventionScenario],
    candidates: &'a [CohomologyInterventionCandidate],
    graphs: Vec<SparseDistanceMatrix>,
    pub(super) spaces: Vec<CohomologySpace>,
    targets: Vec<CohomologyClassId>,
    limits: CohomologyLimits,
}

impl<'a> TopologyOracle<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn build(
        vertex_count: usize,
        dimension: usize,
        scale: f64,
        modulus: u32,
        scenarios: &'a [CohomologyInterventionScenario],
        candidates: &'a [CohomologyInterventionCandidate],
        limits: CohomologyLimits,
    ) -> Result<Self> {
        let mut graphs = Vec::with_capacity(scenarios.len());
        let mut spaces = Vec::with_capacity(scenarios.len());
        let mut targets = Vec::with_capacity(scenarios.len());
        for scenario in scenarios {
            let graph = graph_from_edges(vertex_count, &scenario.active_edges)?;
            let space = cohomology_space(&graph, dimension, scale, modulus, limits)?;
            let target = space
                .basis()
                .get(scenario.target_basis)
                .ok_or_else(|| {
                    Error::InvalidInput(
                        "cohomology intervention target basis is out of range".into(),
                    )
                })?
                .id;
            graphs.push(graph);
            spaces.push(space);
            targets.push(target);
        }
        Ok(Self {
            vertex_count,
            dimension,
            scale,
            modulus,
            scenarios,
            candidates,
            graphs,
            spaces,
            targets,
            limits,
        })
    }

    pub(super) fn survives(&self, selected: &[usize]) -> Result<bool> {
        for scenario in 0..self.scenarios.len() {
            let (graph, space) = self.edited_space(scenario, selected)?;
            let restriction = cohomology_restriction(
                &graph,
                &space,
                &self.graphs[scenario],
                &self.spaces[scenario],
            )?;
            if restriction.image_contains(&self.spaces[scenario], self.targets[scenario])? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(super) fn ranks(&self, selected: &[usize]) -> Result<Vec<usize>> {
        (0..self.scenarios.len())
            .map(|scenario| {
                self.edited_space(scenario, selected)
                    .map(|(_, space)| space.rank())
            })
            .collect()
    }

    fn edited_space(
        &self,
        scenario: usize,
        selected: &[usize],
    ) -> Result<(SparseDistanceMatrix, CohomologySpace)> {
        let mut edges = self.scenarios[scenario].active_edges.clone();
        edges.extend(
            selected
                .iter()
                .map(|position| self.candidates[*position].edge),
        );
        edges.sort();
        let graph = graph_from_edges(self.vertex_count, &edges)?;
        let space = cohomology_space(
            &graph,
            self.dimension,
            self.scale,
            self.modulus,
            self.limits,
        )?;
        Ok((graph, space))
    }
}

pub(super) fn map_status(status: SearchStatus) -> CohomologyInterventionStatus {
    match status {
        SearchStatus::Optimal => CohomologyInterventionStatus::Optimal,
        SearchStatus::Infeasible => CohomologyInterventionStatus::Infeasible,
        SearchStatus::Incomplete => CohomologyInterventionStatus::SearchIncomplete,
    }
}

fn graph_from_edges(vertex_count: usize, edges: &[KineticEdgeKey]) -> Result<SparseDistanceMatrix> {
    let triplets = edges
        .iter()
        .map(|edge| (edge.u, edge.v, 0.0))
        .collect::<Vec<_>>();
    SparseDistanceMatrix::from_triplets(vertex_count, &triplets)
}
