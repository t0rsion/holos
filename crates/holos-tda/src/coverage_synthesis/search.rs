//! Coverage search and proof construction.

use crate::coverage_frontier::{
    CoverageComposition, CoverageCompositionStatus, compose_coverage_frontiers,
};
use crate::monotone_proof::{ProofWork, build_proof};
use crate::monotone_search::{SearchLimits, SearchResult, SearchStatus, minimize_antitone};
use crate::{Error, Result};

use super::evaluate::{survives, validate_actions, validate_source};
use super::model::{
    BuiltCoverageProof, CoverageAction, CoverageSearchData, CoverageSpecification,
    CoverageSynthesisLimits, CoverageSynthesisStatus,
};

pub(super) fn validate_build_inputs(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    limits: CoverageSynthesisLimits,
) -> Result<()> {
    specification.validate(limits.coverage)?;
    validate_source(specification, limits)?;
    validate_actions(specification, actions, limits.coverage)?;
    if limits.max_oracle_calls == 0 || limits.max_search_nodes == 0 {
        Err(Error::InvalidInput(
            "coverage synthesis search limits must be positive".into(),
        ))
    } else {
        Ok(())
    }
}

pub(super) fn solve_coverage_search(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    costs: &[u64],
    max_activations: usize,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageSearchData> {
    if specification.components(actions)?.len() > 1 {
        let composition =
            compose_coverage_frontiers(specification, actions, max_activations, limits)?;
        Ok(composed_search_data(&composition))
    } else {
        let search = minimize_antitone(
            costs,
            max_activations,
            SearchLimits {
                oracle_calls: limits.max_oracle_calls,
                search_nodes: limits.max_search_nodes,
            },
            |selected| survives(specification, actions, selected, limits.coverage),
        )?;
        Ok(monolithic_search_data(search))
    }
}

fn composed_search_data(composition: &CoverageComposition) -> CoverageSearchData {
    let (status, selected, lower_bound, upper_bound) = match composition.status() {
        CoverageCompositionStatus::Optimal => (
            CoverageSynthesisStatus::Optimal,
            composition.selected().to_vec(),
            composition.cost(),
            composition.cost(),
        ),
        CoverageCompositionStatus::Infeasible => {
            (CoverageSynthesisStatus::Infeasible, Vec::new(), None, None)
        }
        CoverageCompositionStatus::SearchIncomplete => (
            CoverageSynthesisStatus::SearchIncomplete,
            Vec::new(),
            Some(0),
            None,
        ),
    };
    CoverageSearchData {
        status,
        selected,
        lower_bound,
        upper_bound,
        oracle_calls: composition.oracle_calls(),
        search_nodes: composition.search_nodes(),
        cache_hits: composition.cache_hits(),
        root_blockers: Vec::new(),
    }
}

fn monolithic_search_data(search: SearchResult) -> CoverageSearchData {
    CoverageSearchData {
        status: coverage_status(search.status),
        selected: search.selected,
        lower_bound: search.lower_bound,
        upper_bound: search.upper_bound,
        oracle_calls: search.oracle_calls,
        search_nodes: search.search_nodes,
        cache_hits: search.cache_hits,
        root_blockers: search.root_blockers,
    }
}

fn coverage_status(status: SearchStatus) -> CoverageSynthesisStatus {
    match status {
        SearchStatus::Optimal => CoverageSynthesisStatus::Optimal,
        SearchStatus::Infeasible => CoverageSynthesisStatus::Infeasible,
        SearchStatus::Incomplete => CoverageSynthesisStatus::SearchIncomplete,
    }
}

pub(super) fn build_coverage_proof(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    costs: &[u64],
    max_activations: usize,
    search: &CoverageSearchData,
    limits: CoverageSynthesisLimits,
) -> Result<BuiltCoverageProof> {
    if search.status == CoverageSynthesisStatus::SearchIncomplete {
        return Ok(BuiltCoverageProof {
            proof: None,
            work: ProofWork::default(),
        });
    }
    let cutoff = coverage_proof_cutoff(search.status, search.upper_bound)?;
    let mut oracle =
        |selected: &[usize]| survives(specification, actions, selected, limits.coverage);
    let (proof, work) = build_proof(
        costs,
        max_activations.min(actions.len()),
        cutoff,
        limits.proof(),
        &mut oracle,
    )?;
    Ok(BuiltCoverageProof {
        proof: Some(proof),
        work,
    })
}

fn coverage_proof_cutoff(
    status: CoverageSynthesisStatus,
    upper_bound: Option<u64>,
) -> Result<Option<u64>> {
    match status {
        CoverageSynthesisStatus::Optimal => upper_bound
            .map(Some)
            .ok_or_else(|| Error::InvalidInput("optimal coverage result has no cost".into())),
        CoverageSynthesisStatus::Infeasible | CoverageSynthesisStatus::SearchIncomplete => Ok(None),
    }
}
