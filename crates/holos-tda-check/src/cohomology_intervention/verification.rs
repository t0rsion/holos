use crate::cohomology::Space;
use crate::{ProofError, ProofLimits};

use super::model::{
    Claim, Scenario, VerifiedCohomologyIntervention, VerifiedCohomologyInterventionStatus,
};
use super::search::{Search, SearchResult, SearchStatus};

pub(super) struct CheckedIntervention {
    search: SearchResult,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
}

pub(super) fn run_independent_search(
    claim: &Claim,
    limits: ProofLimits,
) -> Result<CheckedIntervention, ProofError> {
    let oracle = IndependentOracle::build(claim, limits)?;
    let before_ranks = oracle.spaces.iter().map(Space::rank).collect::<Vec<_>>();
    let costs = claim
        .candidates
        .iter()
        .map(|candidate| candidate.cost)
        .collect::<Vec<_>>();
    let search = Search::new(
        &costs,
        claim.max_edits,
        claim.oracle_limit,
        claim.node_limit,
        |selected| oracle.survives(selected),
    )
    .run()?;
    let after_ranks = if search.selected.is_empty() {
        before_ranks.clone()
    } else {
        oracle.ranks(&search.selected)?
    };
    Ok(CheckedIntervention {
        search,
        before_ranks,
        after_ranks,
    })
}

pub(super) fn verify_search_result(
    claim: &Claim,
    checked: &CheckedIntervention,
) -> Result<(), ProofError> {
    verify_solution(claim, &checked.search)?;
    verify_work(claim, &checked.search)?;
    verify_blocker_result(claim, &checked.search)?;
    verify_rank_result(claim, checked)
}

fn verify_solution(claim: &Claim, search: &SearchResult) -> Result<(), ProofError> {
    if map_status(search.status) != claim.status
        || search.selected != claim.edits
        || search.lower_bound != claim.lower_bound
        || search.upper_bound != claim.upper_bound
    {
        Err(independent_search_error())
    } else {
        Ok(())
    }
}

fn verify_work(claim: &Claim, search: &SearchResult) -> Result<(), ProofError> {
    if search.oracle_calls != claim.oracle_calls
        || search.search_nodes != claim.search_nodes
        || search.cache_hits != claim.cache_hits
    {
        Err(independent_search_error())
    } else {
        Ok(())
    }
}

fn verify_blocker_result(claim: &Claim, search: &SearchResult) -> Result<(), ProofError> {
    if search.root_blockers != claim.root_blockers
        || search.root_blocker_bound != claim.root_blocker_bound
    {
        Err(independent_search_error())
    } else {
        Ok(())
    }
}

fn verify_rank_result(claim: &Claim, checked: &CheckedIntervention) -> Result<(), ProofError> {
    if checked.before_ranks != claim.before_ranks || checked.after_ranks != claim.after_ranks {
        Err(independent_search_error())
    } else {
        Ok(())
    }
}

fn independent_search_error() -> ProofError {
    ProofError::new("cohomology intervention differs from independent weighted search")
}

pub(super) fn intervention_summary(
    claim: &Claim,
    checked: &CheckedIntervention,
) -> VerifiedCohomologyIntervention {
    let total_cost = checked
        .search
        .selected
        .iter()
        .try_fold(0u64, |sum, position| {
            sum.checked_add(claim.candidates[*position].cost)
        });
    VerifiedCohomologyIntervention {
        dimension: claim.dimension,
        modulus: claim.modulus,
        scenarios: claim.scenarios.len(),
        status: claim.status,
        edits: claim.edits.len(),
        total_cost,
        lower_bound_cost: claim.lower_bound,
        upper_bound_cost: claim.upper_bound,
        oracle_calls: claim.oracle_calls,
        search_nodes: claim.search_nodes,
        cache_hits: claim.cache_hits,
        root_blockers: claim.root_blockers.len(),
        root_blocker_bound: claim.root_blocker_bound,
        before_ranks: claim.before_ranks.clone(),
        after_ranks: claim.after_ranks.clone(),
    }
}

struct IndependentOracle<'a> {
    claim: &'a Claim,
    spaces: Vec<Space>,
    limits: ProofLimits,
}

impl<'a> IndependentOracle<'a> {
    fn build(claim: &'a Claim, limits: ProofLimits) -> Result<Self, ProofError> {
        let spaces = claim
            .scenarios
            .iter()
            .map(|scenario| {
                Space::build(
                    claim.vertex_count,
                    claim.dimension,
                    &scenario.edges,
                    claim.modulus,
                    limits,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (scenario, space) in claim.scenarios.iter().zip(&spaces) {
            if scenario.target_basis >= space.rank() {
                return Err(ProofError::new(
                    "cohomology intervention target basis is out of range",
                ));
            }
        }
        Ok(Self {
            claim,
            spaces,
            limits,
        })
    }

    fn survives(&self, selected: &[usize]) -> Result<bool, ProofError> {
        for (position, scenario) in self.claim.scenarios.iter().enumerate() {
            let edited = self.edited_space(scenario, selected)?;
            if self.spaces[position].target_in_image_from(
                &edited,
                scenario.target_basis,
                self.claim.modulus,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ranks(&self, selected: &[usize]) -> Result<Vec<usize>, ProofError> {
        self.claim
            .scenarios
            .iter()
            .map(|scenario| {
                self.edited_space(scenario, selected)
                    .map(|space| space.rank())
            })
            .collect()
    }

    fn edited_space(&self, scenario: &Scenario, selected: &[usize]) -> Result<Space, ProofError> {
        let mut edges = scenario.edges.clone();
        edges.extend(
            selected
                .iter()
                .map(|position| self.claim.candidates[*position].edge),
        );
        edges.sort();
        Space::build(
            self.claim.vertex_count,
            self.claim.dimension,
            &edges,
            self.claim.modulus,
            self.limits,
        )
    }
}

fn map_status(status: SearchStatus) -> VerifiedCohomologyInterventionStatus {
    match status {
        SearchStatus::Optimal => VerifiedCohomologyInterventionStatus::Optimal,
        SearchStatus::Infeasible => VerifiedCohomologyInterventionStatus::Infeasible,
        SearchStatus::Incomplete => VerifiedCohomologyInterventionStatus::SearchIncomplete,
    }
}
