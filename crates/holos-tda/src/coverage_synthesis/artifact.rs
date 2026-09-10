//! Coverage artifact construction and verification.

use crate::monotone_proof::{ProofWork, proof_topology_checks, verify_proof, verify_root_blockers};
use crate::{CoverageLimits, Error, Result};

use super::evaluate::{
    evaluate_coverage_plan, selected_cost, survives, validate_actions, validate_source,
};
use super::model::{
    CoverageAction, CoveragePlanEvaluation, CoverageSpecification, CoverageSynthesisArtifact,
    CoverageSynthesisLimits, CoverageSynthesisStatus, EvaluationClaim,
};
use super::search::{build_coverage_proof, solve_coverage_search, validate_build_inputs};

impl CoverageSynthesisArtifact {
    /// Solve a minimum-cost activation problem and build its proof tree.
    pub fn build(
        specification: CoverageSpecification,
        actions: Vec<CoverageAction>,
        max_activations: usize,
        limits: CoverageSynthesisLimits,
    ) -> Result<Self> {
        validate_build_inputs(&specification, &actions, limits)?;
        let costs = actions.iter().map(|action| action.cost).collect::<Vec<_>>();
        let search =
            solve_coverage_search(&specification, &actions, &costs, max_activations, limits)?;
        let before = evaluate_coverage_plan(&specification, &actions, &[], limits.coverage)?;
        let after =
            evaluate_coverage_plan(&specification, &actions, &search.selected, limits.coverage)?;
        let built = build_coverage_proof(
            &specification,
            &actions,
            &costs,
            max_activations,
            &search,
            limits,
        )?;
        let mut artifact = Self {
            specification,
            actions,
            max_activations,
            oracle_limit: limits.max_oracle_calls,
            node_limit: limits.max_search_nodes,
            status: search.status,
            selected: search.selected,
            lower_bound_cost: search.lower_bound,
            upper_bound_cost: search.upper_bound,
            producer_oracle_calls: search.oracle_calls,
            producer_search_nodes: search.search_nodes,
            producer_cache_hits: search.cache_hits,
            root_blockers: search.root_blockers,
            before: EvaluationClaim::from(&before),
            after: EvaluationClaim::from(&after),
            proof: built.proof,
            proof_work: built.work,
            digest: [0; 32],
        };
        artifact.verify(limits)?;
        artifact.digest = artifact.compute_digest()?;
        Ok(artifact)
    }

    /// Verify the claim and proof tree.
    ///
    /// Verification does not rerun producer search.
    pub fn verify(&self, limits: CoverageSynthesisLimits) -> Result<()> {
        validate_coverage_artifact_inputs(self, limits)?;
        let (_, after) = checked_coverage_evaluations(self, limits.coverage)?;
        let costs = self
            .actions
            .iter()
            .map(|action| action.cost)
            .collect::<Vec<_>>();
        let selected_cost = selected_cost(&costs, &self.selected)?;
        verify_coverage_root(self, &costs, limits)?;
        verify_coverage_status(self, &costs, selected_cost, &after, limits)?;
        verify_coverage_proof_checks(self)
    }

    /// Coverage specification bound to this result.
    pub fn specification(&self) -> &CoverageSpecification {
        &self.specification
    }

    /// Canonical candidate activation list.
    pub fn actions(&self) -> &[CoverageAction] {
        &self.actions
    }

    /// Search completeness status.
    pub fn status(&self) -> CoverageSynthesisStatus {
        self.status
    }

    /// Selected action indices.
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }

    /// Proved lower cost bound, when finite.
    pub fn lower_bound_cost(&self) -> Option<u64> {
        self.lower_bound_cost
    }

    /// Feasible incumbent cost, when present.
    pub fn upper_bound_cost(&self) -> Option<u64> {
        self.upper_bound_cost
    }

    /// Producer oracle calls made during search.
    pub fn producer_oracle_calls(&self) -> usize {
        self.producer_oracle_calls
    }

    /// Producer branch nodes visited during search.
    pub fn producer_search_nodes(&self) -> usize {
        self.producer_search_nodes
    }

    /// Coverage-predicate checks required by the proof tree.
    pub fn proof_topology_checks(&self) -> usize {
        self.proof_work.checks
    }

    /// Proof-tree node count.
    pub fn proof_nodes(&self) -> usize {
        self.proof_work.nodes
    }

    /// Number of state-failure checks for the selected plan.
    pub fn selected_failure_checks(&self) -> usize {
        self.after.checks
    }

    /// Smallest selected-plan witness support across all checked failures.
    pub fn minimum_witness_triangles(&self) -> Option<usize> {
        self.after.minimum_witness_triangles
    }

    /// Content digest of the claim.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

fn validate_coverage_artifact_inputs(
    artifact: &CoverageSynthesisArtifact,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    artifact.specification.validate(limits.coverage)?;
    validate_source(&artifact.specification, limits)?;
    validate_actions(&artifact.specification, &artifact.actions, limits.coverage)?;
    validate_coverage_result_shape(artifact)
}

fn checked_coverage_evaluations(
    artifact: &CoverageSynthesisArtifact,
    limits: CoverageLimits,
) -> Result<(CoveragePlanEvaluation, CoveragePlanEvaluation)> {
    let before = evaluate_coverage_plan(&artifact.specification, &artifact.actions, &[], limits)?;
    let after = evaluate_coverage_plan(
        &artifact.specification,
        &artifact.actions,
        &artifact.selected,
        limits,
    )?;
    verify_evaluation_claims(artifact, &before, &after)?;
    Ok((before, after))
}

fn validate_coverage_result_shape(artifact: &CoverageSynthesisArtifact) -> Result<()> {
    let invalid_selection = artifact.selected.len()
        > artifact.max_activations.min(artifact.actions.len())
        || artifact
            .selected
            .iter()
            .any(|index| *index >= artifact.actions.len())
        || artifact.selected.windows(2).any(|pair| pair[0] >= pair[1]);
    if artifact.oracle_limit == 0
        || artifact.node_limit == 0
        || artifact.producer_oracle_calls > artifact.oracle_limit
        || artifact.producer_search_nodes > artifact.node_limit
        || invalid_selection
    {
        Err(Error::InvalidInput(
            "coverage synthesis result shape or producer work is invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_evaluation_claims(
    artifact: &CoverageSynthesisArtifact,
    before: &CoveragePlanEvaluation,
    after: &CoveragePlanEvaluation,
) -> Result<()> {
    if artifact.before != EvaluationClaim::from(before)
        || artifact.after != EvaluationClaim::from(after)
    {
        Err(Error::InvalidInput(
            "coverage synthesis evaluation claims differ from exact checks".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_coverage_root(
    artifact: &CoverageSynthesisArtifact,
    costs: &[u64],
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    let mut oracle = |selected: &[usize]| {
        survives(
            &artifact.specification,
            &artifact.actions,
            selected,
            limits.coverage,
        )
    };
    verify_root_blockers(
        &artifact.root_blockers,
        costs,
        artifact.lower_bound_cost,
        &mut oracle,
    )
}

fn verify_coverage_status(
    artifact: &CoverageSynthesisArtifact,
    costs: &[u64],
    selected_cost: u64,
    after: &CoveragePlanEvaluation,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    match artifact.status {
        CoverageSynthesisStatus::Optimal => {
            verify_optimal_coverage(artifact, costs, selected_cost, after, limits)
        }
        CoverageSynthesisStatus::Infeasible => {
            verify_infeasible_coverage(artifact, costs, after, limits)
        }
        CoverageSynthesisStatus::SearchIncomplete => {
            verify_incomplete_coverage(artifact, selected_cost, after)
        }
    }
}

fn verify_optimal_coverage(
    artifact: &CoverageSynthesisArtifact,
    costs: &[u64],
    selected_cost: u64,
    after: &CoveragePlanEvaluation,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    if !after.criterion_holds
        || artifact.lower_bound_cost != Some(selected_cost)
        || artifact.upper_bound_cost != Some(selected_cost)
    {
        return Err(Error::InvalidInput(
            "optimal coverage result has an invalid incumbent or bound".into(),
        ));
    }
    verify_checked_coverage_proof(
        artifact,
        costs,
        Some(selected_cost),
        limits,
        "optimal coverage result has no proof tree",
    )
}

fn verify_infeasible_coverage(
    artifact: &CoverageSynthesisArtifact,
    costs: &[u64],
    after: &CoveragePlanEvaluation,
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    if !artifact.selected.is_empty()
        || artifact.lower_bound_cost.is_some()
        || artifact.upper_bound_cost.is_some()
        || after.criterion_holds
    {
        return Err(Error::InvalidInput(
            "infeasible coverage result has an incumbent or finite bound".into(),
        ));
    }
    verify_checked_coverage_proof(
        artifact,
        costs,
        None,
        limits,
        "infeasible coverage result has no proof tree",
    )
}

fn verify_incomplete_coverage(
    artifact: &CoverageSynthesisArtifact,
    selected_cost: u64,
    after: &CoveragePlanEvaluation,
) -> Result<()> {
    let invalid_gap = artifact
        .lower_bound_cost
        .zip(artifact.upper_bound_cost)
        .is_some_and(|(lower, upper)| lower > upper);
    if artifact.proof.is_some()
        || artifact.proof_work != ProofWork::default()
        || artifact.upper_bound_cost.is_some() != after.criterion_holds
        || artifact
            .upper_bound_cost
            .is_some_and(|cost| cost != selected_cost)
        || invalid_gap
    {
        Err(Error::InvalidInput(
            "incomplete coverage result has an invalid gap".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_checked_coverage_proof(
    artifact: &CoverageSynthesisArtifact,
    costs: &[u64],
    cutoff: Option<u64>,
    limits: CoverageSynthesisLimits,
    missing_message: &str,
) -> Result<()> {
    let proof = artifact
        .proof
        .as_ref()
        .ok_or_else(|| Error::InvalidInput(missing_message.into()))?;
    let mut oracle = |selected: &[usize]| {
        survives(
            &artifact.specification,
            &artifact.actions,
            selected,
            limits.coverage,
        )
    };
    let work = verify_proof(
        proof,
        costs,
        artifact.max_activations.min(artifact.actions.len()),
        cutoff,
        limits.proof(),
        &mut oracle,
    )?;
    if work != artifact.proof_work {
        Err(Error::InvalidInput(
            "coverage proof work differs from the checked tree".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_coverage_proof_checks(artifact: &CoverageSynthesisArtifact) -> Result<()> {
    let wrong = artifact
        .proof
        .as_ref()
        .is_some_and(|proof| proof_topology_checks(proof) != artifact.proof_work.checks);
    if wrong {
        Err(Error::InvalidInput(
            "coverage proof check count differs from its tree".into(),
        ))
    } else {
        Ok(())
    }
}
