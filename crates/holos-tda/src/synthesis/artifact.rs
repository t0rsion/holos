use crate::monotone_search::{SearchLimits, SearchStatus, minimize_antitone};
use crate::{Error, Result};

use super::model::{
    ProofNode, SynthesisAction, SynthesisArtifact, SynthesisLimits, SynthesisStatus,
    TopologicalSpecification,
};
use super::oracle::{TopologyOracle, selected_cost, validate_problem, validate_root_blockers};
use super::proof::{ProofVerifier, build_synthesis_proof};

impl SynthesisArtifact {
    /// Solve a minimum-cost action problem and build its proof tree.
    pub fn build(
        specification: TopologicalSpecification,
        actions: Vec<SynthesisAction>,
        max_edits: usize,
        limits: SynthesisLimits,
    ) -> Result<Self> {
        Self::from_problem(
            specification,
            actions,
            max_edits,
            limits.max_oracle_calls,
            limits.max_search_nodes,
            limits,
        )
    }

    fn from_problem(
        specification: TopologicalSpecification,
        actions: Vec<SynthesisAction>,
        max_edits: usize,
        oracle_limit: usize,
        node_limit: usize,
        limits: SynthesisLimits,
    ) -> Result<Self> {
        validate_problem(&specification, &actions, oracle_limit, node_limit, limits)?;
        let oracle = TopologyOracle::build(&specification, &actions, limits.cohomology)?;
        let costs = actions.iter().map(|action| action.cost).collect::<Vec<_>>();
        let search = minimize_antitone(
            &costs,
            max_edits,
            SearchLimits {
                oracle_calls: oracle_limit,
                search_nodes: node_limit,
            },
            |selected| oracle.survives(selected),
        )?;
        let status = synthesis_status(search.status);
        let before_ranks = oracle.target_ranks();
        let after_ranks = oracle.intersection_ranks(&search.selected)?;
        let built = build_synthesis_proof(
            status,
            &search,
            &costs,
            max_edits,
            actions.len(),
            &oracle,
            limits,
        )?;
        let mut artifact = Self {
            specification,
            actions,
            max_edits,
            oracle_limit,
            node_limit,
            status,
            selected: search.selected,
            lower_bound_cost: search.lower_bound,
            upper_bound_cost: search.upper_bound,
            producer_oracle_calls: search.oracle_calls,
            producer_search_nodes: search.search_nodes,
            producer_cache_hits: search.cache_hits,
            root_blockers: search.root_blockers,
            before_ranks,
            after_ranks,
            proof: built.proof,
            proof_nodes: built.nodes,
            proof_topology_checks: built.topology_checks,
            digest: [0; 32],
        };
        artifact.verify(limits)?;
        artifact.digest = artifact.compute_digest()?;
        Ok(artifact)
    }

    /// Verify the claim and proof tree.
    pub fn verify(&self, limits: SynthesisLimits) -> Result<()> {
        validate_problem(
            &self.specification,
            &self.actions,
            self.oracle_limit,
            self.node_limit,
            limits,
        )?;
        verify_result_shape(self)?;
        let oracle = TopologyOracle::build(&self.specification, &self.actions, limits.cohomology)?;
        verify_rank_claims(self, &oracle)?;
        let costs = self
            .actions
            .iter()
            .map(|action| action.cost)
            .collect::<Vec<_>>();
        let selected_cost = selected_cost(&costs, &self.selected)?;
        let selected_feasible = !oracle.survives(&self.selected)?;
        validate_root_blockers(&oracle, &self.root_blockers, &costs, self.lower_bound_cost)?;
        let mut verifier = ProofVerifier::new(
            &costs,
            self.max_edits,
            self.upper_bound_cost,
            &oracle,
            limits,
        );
        verify_status_claim(self, selected_cost, selected_feasible, &mut verifier)?;
        verify_proof_work(self, &verifier)
    }

    /// Finite specification bound to this result.
    pub fn specification(&self) -> &TopologicalSpecification {
        &self.specification
    }

    /// Canonical action list.
    pub fn actions(&self) -> &[SynthesisAction] {
        &self.actions
    }

    /// Search completeness status.
    pub fn status(&self) -> SynthesisStatus {
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

    /// Producer topology calls made by branch-and-bound.
    pub fn producer_oracle_calls(&self) -> usize {
        self.producer_oracle_calls
    }

    /// Producer branch nodes visited by branch-and-bound.
    pub fn producer_search_nodes(&self) -> usize {
        self.producer_search_nodes
    }

    /// Topology checks required by the proof tree.
    pub fn proof_topology_checks(&self) -> usize {
        self.proof_topology_checks
    }

    /// Proof-tree node count.
    pub fn proof_nodes(&self) -> usize {
        self.proof_nodes
    }

    /// Target ranks before editing in state order.
    pub fn before_ranks(&self) -> &[usize] {
        &self.before_ranks
    }

    /// Surviving target ranks after editing in state order.
    pub fn after_ranks(&self) -> &[usize] {
        &self.after_ranks
    }

    /// Content digest of the claim.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
}

fn verify_result_shape(artifact: &SynthesisArtifact) -> Result<()> {
    let invalid_selection = artifact.selected.len() > artifact.max_edits
        || artifact
            .selected
            .iter()
            .any(|index| *index >= artifact.actions.len())
        || artifact.selected.windows(2).any(|pair| pair[0] >= pair[1]);
    if artifact.producer_oracle_calls > artifact.oracle_limit
        || artifact.producer_search_nodes > artifact.node_limit
        || invalid_selection
    {
        Err(Error::InvalidInput(
            "synthesis result shape or producer work is invalid".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_rank_claims(artifact: &SynthesisArtifact, oracle: &TopologyOracle<'_>) -> Result<()> {
    if artifact.before_ranks != oracle.target_ranks()
        || artifact.after_ranks != oracle.intersection_ranks(&artifact.selected)?
    {
        Err(Error::InvalidInput(
            "synthesis rank claims differ from exact restriction images".into(),
        ))
    } else {
        Ok(())
    }
}

fn verify_status_claim(
    artifact: &SynthesisArtifact,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut ProofVerifier<'_>,
) -> Result<()> {
    match artifact.status {
        SynthesisStatus::Optimal => {
            verify_optimal_claim(artifact, selected_cost, selected_feasible, verifier)
        }
        SynthesisStatus::Infeasible => {
            verify_infeasible_claim(artifact, selected_feasible, verifier)
        }
        SynthesisStatus::SearchIncomplete => {
            verify_incomplete_claim(artifact, selected_cost, selected_feasible)
        }
    }
}

fn verify_optimal_claim(
    artifact: &SynthesisArtifact,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut ProofVerifier<'_>,
) -> Result<()> {
    if !selected_feasible
        || artifact.lower_bound_cost != Some(selected_cost)
        || artifact.upper_bound_cost != Some(selected_cost)
    {
        return Err(Error::InvalidInput(
            "optimal synthesis result has an invalid incumbent or bound".into(),
        ));
    }
    verifier.verify_root(required_proof(
        artifact,
        "optimal synthesis result has no proof tree",
    )?)
}

fn verify_infeasible_claim(
    artifact: &SynthesisArtifact,
    selected_feasible: bool,
    verifier: &mut ProofVerifier<'_>,
) -> Result<()> {
    if !artifact.selected.is_empty()
        || artifact.lower_bound_cost.is_some()
        || artifact.upper_bound_cost.is_some()
        || selected_feasible
    {
        return Err(Error::InvalidInput(
            "infeasible synthesis result has an incumbent or finite bound".into(),
        ));
    }
    verifier.verify_root(required_proof(
        artifact,
        "infeasible synthesis result has no proof tree",
    )?)
}

fn verify_incomplete_claim(
    artifact: &SynthesisArtifact,
    selected_cost: u64,
    selected_feasible: bool,
) -> Result<()> {
    let invalid_gap = artifact
        .lower_bound_cost
        .zip(artifact.upper_bound_cost)
        .is_some_and(|(lower, upper)| lower > upper);
    if artifact.proof.is_some()
        || artifact.upper_bound_cost.is_some() != selected_feasible
        || artifact
            .upper_bound_cost
            .is_some_and(|cost| cost != selected_cost)
        || invalid_gap
    {
        Err(Error::InvalidInput(
            "incomplete synthesis result has an invalid gap".into(),
        ))
    } else {
        Ok(())
    }
}

fn required_proof<'a>(artifact: &'a SynthesisArtifact, message: &str) -> Result<&'a ProofNode> {
    artifact
        .proof
        .as_ref()
        .ok_or_else(|| Error::InvalidInput(message.into()))
}

fn verify_proof_work(artifact: &SynthesisArtifact, verifier: &ProofVerifier<'_>) -> Result<()> {
    if artifact.proof_nodes != verifier.nodes || artifact.proof_topology_checks != verifier.checks {
        Err(Error::InvalidInput(
            "synthesis proof work differs from the checked tree".into(),
        ))
    } else {
        Ok(())
    }
}

fn synthesis_status(status: SearchStatus) -> SynthesisStatus {
    match status {
        SearchStatus::Optimal => SynthesisStatus::Optimal,
        SearchStatus::Infeasible => SynthesisStatus::Infeasible,
        SearchStatus::Incomplete => SynthesisStatus::SearchIncomplete,
    }
}
