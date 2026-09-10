use crate::{ProofError, ProofLimits};

use super::MAGIC;
use super::evaluate::{blocker_bound, evaluate, selected_cost};
use super::model::{
    CheckedCoverage, Claim, CoverageGeometryClaim, DecodedCoverage, Evaluation, Source,
    VerifiedCoverage, VerifiedCoverageSource, VerifiedCoverageStatus,
};
use super::proof::{TreeVerifier, verify_root_blockers};
use super::source::validate_source;
use super::wire::decode_coverage;

/// Return true when bytes start with a coverage envelope.
pub fn is_coverage(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify one bounded `HOLOSCOV` coverage proof.
pub fn verify_coverage(bytes: &[u8], limits: ProofLimits) -> Result<VerifiedCoverage, ProofError> {
    let decoded = decode_coverage(bytes, limits)?;
    let checked = verify_claim(&decoded, limits)?;
    Ok(coverage_summary(&decoded, checked))
}

pub(crate) fn verify_coverage_with_geometry_claim(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<(VerifiedCoverage, CoverageGeometryClaim), ProofError> {
    let decoded = decode_coverage(bytes, limits)?;
    let checked = verify_claim(&decoded, limits)?;
    if !matches!(decoded.claim.source, Source::Finite) {
        return Err(ProofError::new(
            "geometry binding accepts finite coverage states only",
        ));
    }
    let summary = coverage_summary(&decoded, checked);
    let claim = CoverageGeometryClaim {
        vertex_count: decoded.claim.vertex_count,
        broadcast_radius: decoded.claim.broadcast_radius,
        sensing_radius: decoded.claim.sensing_radius,
        fence: decoded.claim.fence.clone(),
        state_edges: decoded
            .claim
            .states
            .iter()
            .map(|state| state.edges.iter().map(|edge| (edge.u, edge.v)).collect())
            .collect(),
    };
    Ok((summary, claim))
}

pub(crate) fn verify_claim(
    decoded: &DecodedCoverage,
    limits: ProofLimits,
) -> Result<CheckedCoverage, ProofError> {
    let claim = &decoded.claim;
    validate_source(claim, limits)?;
    let checked_before = evaluate(claim, &[], limits)?;
    let checked_after = evaluate(claim, &claim.selected, limits)?;
    if claim.before != checked_before || claim.after != checked_after {
        return Err(ProofError::new(
            "coverage evaluation claims differ from exact checks",
        ));
    }
    let costs = claim
        .actions
        .iter()
        .map(|action| action.cost)
        .collect::<Vec<_>>();
    let selected_cost = selected_cost(&costs, &claim.selected)?;
    verify_root_blockers(
        claim,
        &claim.root_blockers,
        &costs,
        claim.lower_bound,
        limits,
    )?;
    let mut verifier = TreeVerifier::new(
        &costs,
        claim.max_activations.min(claim.actions.len()),
        verifier_incumbent(claim, selected_cost),
        claim,
        limits,
    );
    verify_status(decoded, checked_after, selected_cost, &costs, &mut verifier)?;
    verify_proof_work(decoded, &verifier)?;
    Ok(CheckedCoverage {
        after: checked_after,
        selected_cost,
    })
}

pub(crate) fn verifier_incumbent(claim: &Claim, selected_cost: u64) -> Option<u64> {
    match claim.status {
        VerifiedCoverageStatus::Optimal => Some(selected_cost),
        VerifiedCoverageStatus::Infeasible => None,
        VerifiedCoverageStatus::SearchIncomplete => claim.upper_bound,
    }
}

pub(crate) fn verify_status(
    decoded: &DecodedCoverage,
    checked_after: Evaluation,
    selected_cost: u64,
    costs: &[u64],
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    match decoded.claim.status {
        VerifiedCoverageStatus::Optimal => {
            verify_optimal(&decoded.claim, checked_after, selected_cost, verifier)
        }
        VerifiedCoverageStatus::Infeasible => {
            verify_infeasible(&decoded.claim, checked_after, verifier)
        }
        VerifiedCoverageStatus::SearchIncomplete => {
            verify_incomplete(decoded, checked_after, selected_cost, costs)
        }
    }
}

pub(crate) fn verify_optimal(
    claim: &Claim,
    checked_after: Evaluation,
    selected_cost: u64,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !checked_after.criterion_holds
        || claim.lower_bound != Some(selected_cost)
        || claim.upper_bound != Some(selected_cost)
    {
        return Err(ProofError::new(
            "optimal coverage result has an invalid incumbent or bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("optimal coverage result has no proof tree"))?;
    verifier.verify_root(proof)
}

pub(crate) fn verify_infeasible(
    claim: &Claim,
    checked_after: Evaluation,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !claim.selected.is_empty()
        || claim.lower_bound.is_some()
        || claim.upper_bound.is_some()
        || checked_after.criterion_holds
    {
        return Err(ProofError::new(
            "infeasible coverage result has an incumbent or finite bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("infeasible coverage result has no proof tree"))?;
    verifier.verify_root(proof)
}

pub(crate) fn verify_incomplete(
    decoded: &DecodedCoverage,
    checked_after: Evaluation,
    selected_cost: u64,
    costs: &[u64],
) -> Result<(), ProofError> {
    let claim = &decoded.claim;
    let root_bound = blocker_bound(costs, &claim.root_blockers)?;
    if claim.proof.is_some()
        || decoded.proof_nodes != 0
        || decoded.proof_topology_checks != 0
        || decoded.proof_terms != 0
        || claim.upper_bound.is_some() != checked_after.criterion_holds
        || claim.upper_bound.is_some_and(|cost| cost != selected_cost)
        || claim.lower_bound != Some(root_bound)
        || claim
            .lower_bound
            .zip(claim.upper_bound)
            .is_some_and(|(lower, upper)| lower > upper)
    {
        return Err(ProofError::new(
            "incomplete coverage result has an invalid gap",
        ));
    }
    Ok(())
}

pub(crate) fn verify_proof_work(
    decoded: &DecodedCoverage,
    verifier: &TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if verifier.nodes != decoded.proof_nodes
        || verifier.checks != decoded.proof_topology_checks
        || verifier.terms != decoded.proof_terms
    {
        return Err(ProofError::new(
            "coverage proof work differs from the checked tree",
        ));
    }
    Ok(())
}

pub(crate) fn coverage_summary(
    decoded: &DecodedCoverage,
    checked: CheckedCoverage,
) -> VerifiedCoverage {
    let claim = &decoded.claim;
    VerifiedCoverage {
        modulus: claim.modulus,
        source: match claim.source {
            Source::Finite => VerifiedCoverageSource::Finite,
            Source::Affine { .. } => VerifiedCoverageSource::Affine,
        },
        states: claim.states.len(),
        actions: claim.actions.len(),
        failure_budget: claim.failure_budget,
        status: claim.status,
        selected: claim.selected.len(),
        total_cost: checked
            .after
            .criterion_holds
            .then_some(checked.selected_cost),
        lower_bound_cost: claim.lower_bound,
        upper_bound_cost: claim.upper_bound,
        producer_oracle_calls: decoded.producer_oracle_calls,
        producer_search_nodes: decoded.producer_search_nodes,
        proof_nodes: decoded.proof_nodes,
        proof_topology_checks: decoded.proof_topology_checks,
        selected_failure_checks: checked.after.checks,
        minimum_witness_triangles: checked.after.minimum_witness,
    }
}
