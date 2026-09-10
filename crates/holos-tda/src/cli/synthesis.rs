//! Cohomology intervention and topological synthesis workflows.

use std::path::Path;

use crate::io;
use crate::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyLimits, KineticFiltration, KineticLimits,
    SparseDistanceMatrix, SynthesisAction, SynthesisArtifact, SynthesisLimits, SynthesisState,
    TopologicalSpecification, cohomology_space,
};

use super::args::{CohomologyInterventionCli, KineticSynthesisCli, LinkPlanCli, SynthesisCli};
use super::input::{read_kinetic_edges, read_proof_input, write_via_temporary};

pub(super) fn run_cohomology_intervention(cli: CohomologyInterventionCli) -> crate::Result<()> {
    let graph = read_proof_input(&cli.input, cli.format, cli.threads, Some(cli.scale))?;
    let candidates = parse_weighted_candidates(&cli.candidates)?;
    let limits = CohomologyInterventionLimits {
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..CohomologyInterventionLimits::default()
    };
    let scenario = CohomologyInterventionScenario::from_graph(&graph, cli.scale, cli.target)?;
    let artifact = CohomologyInterventionArtifact::build(
        graph.len(),
        cli.dimension,
        cli.scale,
        cli.modulus,
        &[scenario],
        &candidates,
        cli.max_edits,
        limits,
    )?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} intervention: {}, {} edits, {} oracle calls, cost bounds {:?} to {:?}, wrote {} bytes",
        cli.dimension,
        artifact.status(),
        artifact.edits().len(),
        artifact.oracle_calls(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        bytes.len()
    );
    Ok(())
}

pub(super) fn run_link_plan(cli: LinkPlanCli) -> crate::Result<()> {
    if cli.scenarios.len() != cli.targets.len() {
        return Err(crate::Error::InvalidInput(
            "--scenario and --target counts must match".into(),
        ));
    }
    let scenarios = cli
        .scenarios
        .iter()
        .zip(&cli.targets)
        .map(|(path, target)| {
            let parsed = io::read_sparse_matrix(path, cli.threads)?;
            if parsed.len() > cli.vertices {
                return Err(crate::Error::InvalidInput(format!(
                    "scenario {} uses a vertex above --vertices {}",
                    path.display(),
                    cli.vertices
                )));
            }
            let triplets = parsed.edges().collect::<Vec<_>>();
            let graph = SparseDistanceMatrix::from_triplets(cli.vertices, &triplets)?;
            CohomologyInterventionScenario::from_graph(&graph, cli.scale, *target)
        })
        .collect::<crate::Result<Vec<_>>>()?;
    let candidates = parse_weighted_candidates(&cli.candidates)?;
    let limits = CohomologyInterventionLimits {
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..CohomologyInterventionLimits::default()
    };
    let artifact = CohomologyInterventionArtifact::build(
        cli.vertices,
        cli.dimension,
        cli.scale,
        cli.modulus,
        &scenarios,
        &candidates,
        cli.max_edits,
        limits,
    )?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} link plan across {} scenarios: {}, {} links, cost bounds {:?} to {:?}, {} oracle calls, wrote {} bytes",
        cli.dimension,
        scenarios.len(),
        artifact.status(),
        artifact.edits().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.oracle_calls(),
        bytes.len(),
    );
    Ok(())
}

pub(super) fn run_synthesis(cli: SynthesisCli) -> crate::Result<()> {
    let declared_states = cli.states.len();
    let states = synthesis_states(&cli)?;
    let specification =
        TopologicalSpecification::new(cli.vertices, cli.dimension, cli.scale, cli.modulus, states);
    let actions = parse_synthesis_actions(&cli.candidates, &specification)?;
    let limits = SynthesisLimits {
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..SynthesisLimits::default()
    };
    let artifact = SynthesisArtifact::build(specification, actions, cli.max_edits, limits)?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} synthesis across {} of {} constrained states: {}, {} actions, cost bounds {:?} to {:?}, {} producer topology calls, {} proof topology checks, wrote {} bytes",
        cli.dimension,
        artifact.specification().states().len(),
        declared_states,
        artifact.status(),
        artifact.selected().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.proof_topology_checks(),
        bytes.len(),
    );
    Ok(())
}

pub(super) fn synthesis_states(cli: &SynthesisCli) -> crate::Result<Vec<SynthesisState>> {
    let mut states = Vec::new();
    for (step, path) in cli.states.iter().enumerate() {
        let graph = read_synthesis_state(path, cli.vertices, cli.threads)?;
        let space = cohomology_space(
            &graph,
            cli.dimension,
            cli.scale,
            cli.modulus,
            CohomologyLimits::default(),
        )?;
        if space.rank() <= cli.max_rank {
            continue;
        }
        let target = space.full_subspace();
        states.push(SynthesisState::from_subspace(
            0,
            step as u64,
            &graph,
            cli.scale,
            &space,
            &target,
            cli.max_rank,
        )?);
    }
    Ok(states)
}

pub(super) fn read_synthesis_state(
    path: &Path,
    vertices: usize,
    threads: usize,
) -> crate::Result<SparseDistanceMatrix> {
    let parsed = io::read_sparse_matrix(path, threads)?;
    if parsed.len() > vertices {
        return Err(crate::Error::InvalidInput(format!(
            "state {} uses a vertex above --vertices {vertices}",
            path.display()
        )));
    }
    SparseDistanceMatrix::from_triplets(vertices, &parsed.edges().collect::<Vec<_>>())
}

pub(super) fn run_kinetic_synthesis(cli: KineticSynthesisCli) -> crate::Result<()> {
    let edges = read_kinetic_edges(&cli.input, cli.max_artifact_bytes)?;
    let trajectory = KineticFiltration::new(
        cli.vertices,
        edges,
        cli.start,
        cli.end,
        KineticLimits::default(),
    )?;
    let specification = TopologicalSpecification::from_kinetic_rank_ceiling(
        &trajectory,
        0,
        cli.dimension,
        cli.scale,
        cli.modulus,
        cli.max_rank,
        CohomologyLimits::default(),
    )?;
    let actions = parse_synthesis_actions(&cli.candidates, &specification)?;
    let limits = SynthesisLimits {
        max_bytes: cli.max_artifact_bytes,
        max_oracle_calls: cli.oracle_limit,
        max_search_nodes: cli.node_limit,
        ..SynthesisLimits::default()
    };
    let artifact = SynthesisArtifact::build(specification, actions, cli.max_edits, limits)?;
    let bytes = artifact.encode(limits)?;
    write_via_temporary(&cli.output, &bytes)?;
    println!(
        "certified H{} all-time Rips rank plan across {} constrained critical states: {}, {} actions, cost bounds {:?} to {:?}, {} producer topology calls, {} proof topology checks, wrote {} bytes",
        cli.dimension,
        artifact.specification().states().len(),
        artifact.status(),
        artifact.selected().len(),
        artifact.lower_bound_cost(),
        artifact.upper_bound_cost(),
        artifact.producer_oracle_calls(),
        artifact.proof_topology_checks(),
        bytes.len(),
    );
    Ok(())
}

pub(super) fn parse_synthesis_actions(
    values: &[usize],
    specification: &TopologicalSpecification,
) -> crate::Result<Vec<SynthesisAction>> {
    if specification.states().is_empty() {
        return Ok(Vec::new());
    }
    let mut actions = values
        .chunks_exact(3)
        .map(|candidate| {
            SynthesisAction::throughout(
                candidate[0],
                candidate[1],
                candidate[2] as u64,
                specification,
            )
        })
        .collect::<Vec<_>>();
    actions.sort();
    let original_count = actions.len();
    actions.dedup_by_key(|action| action.edge);
    if actions.len() != original_count {
        return Err(crate::Error::InvalidInput(
            "--candidate repeats an edge".into(),
        ));
    }
    Ok(actions)
}

pub(super) fn parse_weighted_candidates(
    values: &[usize],
) -> crate::Result<Vec<CohomologyInterventionCandidate>> {
    let mut candidates = values
        .chunks_exact(3)
        .map(|candidate| {
            CohomologyInterventionCandidate::new(candidate[0], candidate[1], candidate[2] as u64)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| candidate.edge);
    let original_count = candidates.len();
    candidates.dedup_by_key(|candidate| candidate.edge);
    if candidates.len() != original_count {
        return Err(crate::Error::InvalidInput(
            "--candidate repeats an edge".into(),
        ));
    }
    Ok(candidates)
}
