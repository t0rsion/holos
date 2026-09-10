use std::collections::BTreeSet;

use crate::{Error, KineticEdgeKey, Result};

use super::model::{
    CohomologyInterventionArtifact, CohomologyInterventionCandidate, CohomologyInterventionLimits,
    CohomologyInterventionScenario, CohomologyInterventionStatus, FORMAT_MAX_CANDIDATES,
    FORMAT_MAX_ORACLE_CALLS, FORMAT_MAX_PROOF_TERMS, FORMAT_MAX_SCENARIOS, FORMAT_MAX_SEARCH_NODES,
};

pub(super) fn validate_claim_shape(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if artifact.before_ranks.len() != artifact.scenarios.len()
        || artifact.after_ranks.len() != artifact.scenarios.len()
        || artifact.oracle_calls > artifact.oracle_limit
        || artifact.search_nodes > artifact.node_limit
    {
        Err(Error::InvalidInput(
            "cohomology intervention claim shape is invalid".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_edits(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    let positions = artifact
        .edits
        .iter()
        .map(|edit| {
            artifact.candidates.binary_search(edit).map_err(|_| {
                Error::InvalidInput("cohomology intervention edit is not a candidate".into())
            })
        })
        .collect::<Result<Vec<_>>>()?;
    if positions.windows(2).any(|pair| pair[0] >= pair[1])
        || artifact.edits.len() > artifact.max_edits
    {
        Err(Error::InvalidInput(
            "cohomology intervention edit list is not canonical".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_root_blocker_claim(
    artifact: &CohomologyInterventionArtifact,
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut proof_terms = 0usize;
    let mut bound = 0u64;
    for blocker in &artifact.root_blockers {
        validate_root_blocker(blocker, artifact.candidates.len(), &mut seen)?;
        proof_terms = proof_terms.checked_add(blocker.len()).ok_or_else(|| {
            Error::InvalidInput("cohomology intervention proof term count overflows".into())
        })?;
        let minimum = blocker
            .iter()
            .map(|position| artifact.candidates[*position].cost)
            .min()
            .expect("a checked blocker is nonempty");
        bound = bound.checked_add(minimum).ok_or_else(|| {
            Error::InvalidInput("cohomology intervention blocker bound overflows".into())
        })?;
    }
    if proof_terms > limits.max_proof_terms.min(FORMAT_MAX_PROOF_TERMS)
        || bound != artifact.root_blocker_bound
    {
        Err(Error::InvalidInput(
            "cohomology intervention blocker claim is invalid".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_root_blocker(
    blocker: &[usize],
    candidate_count: usize,
    seen: &mut BTreeSet<usize>,
) -> Result<()> {
    if blocker.is_empty()
        || blocker.windows(2).any(|pair| pair[0] >= pair[1])
        || blocker.iter().any(|position| *position >= candidate_count)
        || blocker.iter().any(|position| !seen.insert(*position))
    {
        Err(Error::InvalidInput(
            "cohomology intervention root blockers are not canonical and disjoint".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_status_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    match artifact.status {
        CohomologyInterventionStatus::Optimal => validate_optimal_claim(artifact),
        CohomologyInterventionStatus::Infeasible => validate_infeasible_claim(artifact),
        CohomologyInterventionStatus::SearchIncomplete => validate_incomplete_claim(artifact),
    }
}

pub(super) fn validate_optimal_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if artifact.lower_bound_cost.is_none()
        || artifact.lower_bound_cost != artifact.upper_bound_cost
        || artifact.edits.is_empty() && artifact.upper_bound_cost != Some(0)
    {
        Err(Error::InvalidInput(
            "optimal intervention bounds are invalid".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_infeasible_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if !artifact.edits.is_empty()
        || artifact.lower_bound_cost.is_some()
        || artifact.upper_bound_cost.is_some()
    {
        Err(Error::InvalidInput(
            "infeasible intervention carries a finite bound".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_incomplete_claim(artifact: &CohomologyInterventionArtifact) -> Result<()> {
    if artifact.lower_bound_cost.is_none()
        || artifact
            .lower_bound_cost
            .zip(artifact.upper_bound_cost)
            .is_some_and(|(lower, upper)| lower > upper)
    {
        Err(Error::InvalidInput(
            "incomplete intervention bounds are invalid".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn validate_decoded_artifact(
    artifact: &CohomologyInterventionArtifact,
    bytes: &[u8],
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    artifact.validate_claim(limits)?;
    if artifact.compute_digest()? != artifact.digest {
        return Err(Error::InvalidInput(
            "cohomology intervention digest differs from its content".into(),
        ));
    }
    artifact.verify(limits)?;
    if artifact.encode(limits)? != bytes {
        return Err(Error::InvalidInput(
            "cohomology intervention encoding is not canonical".into(),
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn validate_problem(
    vertex_count: usize,
    scale: f64,
    scenarios: &[CohomologyInterventionScenario],
    candidates: &[CohomologyInterventionCandidate],
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    validate_problem_scope(vertex_count, scale, scenarios, limits)?;
    for scenario in scenarios {
        validate_edges(
            vertex_count,
            &scenario.active_edges,
            limits.max_edges_per_scenario,
            "active edge",
        )?;
    }
    validate_candidates(vertex_count, candidates, limits)?;
    validate_inactive_candidates(scenarios, candidates)?;
    validate_search_limits(
        max_edits,
        candidates.len(),
        oracle_limit,
        node_limit,
        limits,
    )
}

pub(super) fn validate_problem_scope(
    vertex_count: usize,
    scale: f64,
    scenarios: &[CohomologyInterventionScenario],
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    if vertex_count > limits.max_vertices {
        return Err(Error::InvalidInput(
            "cohomology intervention vertex count exceeds its limit".into(),
        ));
    }
    if !scale.is_finite() || scale < 0.0 {
        return Err(Error::InvalidInput(
            "cohomology intervention scale must be finite and non-negative".into(),
        ));
    }
    if scenarios.is_empty() || scenarios.len() > limits.max_scenarios.min(FORMAT_MAX_SCENARIOS) {
        return Err(Error::InvalidInput(
            "cohomology intervention scenario count is invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_candidates(
    vertex_count: usize,
    candidates: &[CohomologyInterventionCandidate],
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    if candidates.len() > limits.max_candidates.min(FORMAT_MAX_CANDIDATES)
        || candidates.iter().any(|candidate| candidate.cost == 0)
        || candidates.iter().any(|candidate| {
            candidate.edge.u >= candidate.edge.v || candidate.edge.v >= vertex_count
        })
        || candidates
            .windows(2)
            .any(|pair| pair[0].edge >= pair[1].edge)
    {
        return Err(Error::InvalidInput(
            "cohomology intervention candidate list is not canonical".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_inactive_candidates(
    scenarios: &[CohomologyInterventionScenario],
    candidates: &[CohomologyInterventionCandidate],
) -> Result<()> {
    for scenario in scenarios {
        let active = scenario
            .active_edges
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        if candidates
            .iter()
            .any(|candidate| active.contains(&candidate.edge))
        {
            return Err(Error::InvalidInput(
                "cohomology intervention candidate is active in a scenario".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_search_limits(
    max_edits: usize,
    candidate_count: usize,
    oracle_limit: usize,
    node_limit: usize,
    limits: CohomologyInterventionLimits,
) -> Result<()> {
    if max_edits > candidate_count {
        return Err(Error::InvalidInput(
            "cohomology intervention edit limit exceeds the candidate count".into(),
        ));
    }
    if oracle_limit == 0
        || oracle_limit > limits.max_oracle_calls.min(FORMAT_MAX_ORACLE_CALLS)
        || node_limit == 0
        || node_limit > limits.max_search_nodes.min(FORMAT_MAX_SEARCH_NODES)
    {
        return Err(Error::InvalidInput(
            "cohomology intervention search limits are invalid".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_edges(
    vertex_count: usize,
    edges: &[KineticEdgeKey],
    maximum: usize,
    name: &str,
) -> Result<()> {
    if edges.len() > maximum
        || edges
            .iter()
            .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(Error::InvalidInput(format!(
            "cohomology intervention {name} list is not canonical or exceeds its limit"
        )));
    }
    Ok(())
}
