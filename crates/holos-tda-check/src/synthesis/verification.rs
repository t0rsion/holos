use crate::{ProofError, ProofLimits};

use super::model::{
    CheckedSynthesis, Claim, DecodedSynthesis, Source, VerifiedSynthesis, VerifiedSynthesisSource,
    VerifiedSynthesisStatus,
};
use super::oracle::{Oracle, validate_source};
use super::proof::{TreeVerifier, selected_cost, verify_root_blockers};

pub(super) fn verify_claim(
    decoded: &DecodedSynthesis,
    limits: ProofLimits,
) -> Result<CheckedSynthesis, ProofError> {
    let claim = &decoded.claim;
    validate_source(claim, limits)?;
    let oracle = Oracle::build(claim, limits)?;
    verify_rank_claims(claim, &oracle)?;
    let costs = claim
        .actions
        .iter()
        .map(|action| action.cost)
        .collect::<Vec<_>>();
    let selected_cost = selected_cost(&costs, &claim.selected)?;
    let selected_feasible = !oracle.survives(&claim.selected)?;
    verify_root_blockers(&oracle, &claim.root_blockers, &costs, claim.lower_bound)?;
    let mut verifier =
        TreeVerifier::new(&costs, claim.max_edits, claim.upper_bound, &oracle, limits);
    verify_status(claim, selected_cost, selected_feasible, &mut verifier)?;
    verify_proof_work(decoded, &verifier)?;
    Ok(CheckedSynthesis {
        selected_cost,
        selected_feasible,
    })
}

fn verify_rank_claims(claim: &Claim, oracle: &Oracle<'_>) -> Result<(), ProofError> {
    let before = oracle.target_ranks();
    let after = oracle.intersection_ranks(&claim.selected)?;
    if claim.before_ranks != before || claim.after_ranks != after {
        Err(ProofError::new(
            "synthesis rank claims differ from exact restriction images",
        ))
    } else {
        Ok(())
    }
}

fn verify_status(
    claim: &Claim,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    match claim.status {
        VerifiedSynthesisStatus::Optimal => {
            verify_optimal(claim, selected_cost, selected_feasible, verifier)
        }
        VerifiedSynthesisStatus::Infeasible => {
            verify_infeasible(claim, selected_feasible, verifier)
        }
        VerifiedSynthesisStatus::SearchIncomplete => {
            verify_incomplete(claim, selected_cost, selected_feasible)
        }
    }
}

fn verify_optimal(
    claim: &Claim,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !selected_feasible
        || claim.lower_bound != Some(selected_cost)
        || claim.upper_bound != Some(selected_cost)
    {
        return Err(ProofError::new(
            "optimal synthesis result has an invalid incumbent or bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("optimal synthesis result has no proof tree"))?;
    verifier.verify_root(proof)
}

fn verify_infeasible(
    claim: &Claim,
    selected_feasible: bool,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !claim.selected.is_empty()
        || claim.lower_bound.is_some()
        || claim.upper_bound.is_some()
        || selected_feasible
    {
        return Err(ProofError::new(
            "infeasible synthesis result has an incumbent or finite bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("infeasible synthesis result has no proof tree"))?;
    verifier.verify_root(proof)
}

fn verify_incomplete(
    claim: &Claim,
    selected_cost: u64,
    selected_feasible: bool,
) -> Result<(), ProofError> {
    if claim.proof.is_some()
        || claim.upper_bound.is_some() != selected_feasible
        || claim.upper_bound.is_some_and(|cost| cost != selected_cost)
        || claim
            .lower_bound
            .zip(claim.upper_bound)
            .is_some_and(|(lower, upper)| lower > upper)
    {
        Err(ProofError::new(
            "incomplete synthesis result has an invalid gap",
        ))
    } else {
        Ok(())
    }
}

fn verify_proof_work(
    decoded: &DecodedSynthesis,
    verifier: &TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if verifier.nodes != decoded.proof_nodes || verifier.checks != decoded.proof_topology_checks {
        Err(ProofError::new(
            "synthesis proof work differs from the checked tree",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn synthesis_summary(
    decoded: &DecodedSynthesis,
    checked: CheckedSynthesis,
) -> VerifiedSynthesis {
    let claim = &decoded.claim;
    VerifiedSynthesis {
        dimension: claim.dimension,
        modulus: claim.modulus,
        source: match claim.source {
            Source::Finite => VerifiedSynthesisSource::Finite,
            Source::Affine { .. } => VerifiedSynthesisSource::Affine,
        },
        states: claim.states.len(),
        actions: claim.actions.len(),
        status: claim.status,
        selected: claim.selected.len(),
        total_cost: checked.selected_feasible.then_some(checked.selected_cost),
        lower_bound_cost: claim.lower_bound,
        upper_bound_cost: claim.upper_bound,
        producer_oracle_calls: decoded.producer_oracle_calls,
        producer_search_nodes: decoded.producer_search_nodes,
        proof_nodes: decoded.proof_nodes,
        proof_topology_checks: decoded.proof_topology_checks,
        before_ranks: claim.before_ranks.clone(),
        after_ranks: claim.after_ranks.clone(),
    }
}
