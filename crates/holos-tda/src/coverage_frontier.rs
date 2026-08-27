//! Exact component frontiers for failure-tolerant coverage synthesis.

use std::collections::BTreeMap;
use std::fmt;

use crate::coverage_synthesis::evaluate_coverage_plan_states_prevalidated;
use crate::monotone_search::{SearchLimits, SearchResult, SearchStatus, minimize_antitone};
use crate::{
    CoverageAction, CoverageComponent, CoverageSpecification, CoverageSynthesisLimits, Error,
    Result,
};

/// Completeness status of a compositional coverage calculation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverageCompositionStatus {
    /// Every component frontier is complete, and the returned plan is optimal.
    Optimal,
    /// Every component frontier is complete, but no plan meets the activation limit.
    Infeasible,
    /// A producer work limit stopped at least one component search.
    SearchIncomplete,
}

impl fmt::Display for CoverageCompositionStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Optimal => "optimal",
            Self::Infeasible => "infeasible",
            Self::SearchIncomplete => "search incomplete",
        })
    }
}

/// One nondominated local plan on a component frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageFrontierEntry {
    activations: usize,
    cost: u64,
    selected: Vec<usize>,
}

impl CoverageFrontierEntry {
    /// Number of selected actions in this local plan.
    pub fn activations(&self) -> usize {
        self.activations
    }

    /// Total action cost of this local plan.
    pub fn cost(&self) -> u64 {
        self.cost
    }

    /// Selected global action indices in canonical order.
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }
}

/// Complete nondominated plan frontier for one incidence component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageComponentFrontier {
    component: CoverageComponent,
    entries: Vec<CoverageFrontierEntry>,
}

impl CoverageComponentFrontier {
    /// State-action incidence component represented by this frontier.
    pub fn component(&self) -> &CoverageComponent {
        &self.component
    }

    /// Nondominated local plans ordered by activation count.
    pub fn entries(&self) -> &[CoverageFrontierEntry] {
        &self.entries
    }
}

/// Result of exact frontier construction and dynamic-program composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverageComposition {
    status: CoverageCompositionStatus,
    frontiers: Vec<CoverageComponentFrontier>,
    selected: Vec<usize>,
    cost: Option<u64>,
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
}

impl CoverageComposition {
    /// Completeness status of the composed calculation.
    pub fn status(&self) -> CoverageCompositionStatus {
        self.status
    }

    /// Component frontiers completed before the reported status.
    pub fn frontiers(&self) -> &[CoverageComponentFrontier] {
        &self.frontiers
    }

    /// Selected global action indices for an optimal plan.
    pub fn selected(&self) -> &[usize] {
        &self.selected
    }

    /// Optimal cost, when a feasible plan was proved.
    pub fn cost(&self) -> Option<u64> {
        self.cost
    }

    /// Coverage predicate calls made by all local searches.
    pub fn oracle_calls(&self) -> usize {
        self.oracle_calls
    }

    /// Branch nodes visited by all local searches.
    pub fn search_nodes(&self) -> usize {
        self.search_nodes
    }

    /// Reused local predicate results across all local searches.
    pub fn cache_hits(&self) -> usize {
        self.cache_hits
    }
}

/// Build exact component frontiers and compose a minimum-cost global plan.
///
/// The state-action incidence graph defines the components. Each local search
/// covers every activation limit that can contribute to the global plan. The
/// final dynamic program enforces `max_activations` across all components.
pub fn compose_coverage_frontiers(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    max_activations: usize,
    limits: CoverageSynthesisLimits,
) -> Result<CoverageComposition> {
    if limits.max_oracle_calls == 0 || limits.max_search_nodes == 0 {
        return Err(Error::InvalidInput(
            "coverage composition search limits must be positive".into(),
        ));
    }
    let components = specification.components(actions)?;
    let mut work = CompositionWork::default();
    let mut frontiers = Vec::with_capacity(components.len());
    for component in components {
        let Some(frontier) = build_frontier(
            specification,
            actions,
            component,
            max_activations.min(actions.len()),
            limits,
            &mut work,
        )?
        else {
            return Ok(CoverageComposition {
                status: CoverageCompositionStatus::SearchIncomplete,
                frontiers,
                selected: Vec::new(),
                cost: None,
                oracle_calls: work.oracle_calls,
                search_nodes: work.search_nodes,
                cache_hits: work.cache_hits,
            });
        };
        frontiers.push(frontier);
    }
    let Some((cost, selected)) = compose(&frontiers, max_activations.min(actions.len()))? else {
        return Ok(CoverageComposition {
            status: CoverageCompositionStatus::Infeasible,
            frontiers,
            selected: Vec::new(),
            cost: None,
            oracle_calls: work.oracle_calls,
            search_nodes: work.search_nodes,
            cache_hits: work.cache_hits,
        });
    };
    Ok(CoverageComposition {
        status: CoverageCompositionStatus::Optimal,
        frontiers,
        selected,
        cost: Some(cost),
        oracle_calls: work.oracle_calls,
        search_nodes: work.search_nodes,
        cache_hits: work.cache_hits,
    })
}

#[derive(Debug, Default)]
struct CompositionWork {
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
}

fn build_frontier(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    component: CoverageComponent,
    global_limit: usize,
    limits: CoverageSynthesisLimits,
    work: &mut CompositionWork,
) -> Result<Option<CoverageComponentFrontier>> {
    let local_costs = component
        .actions()
        .iter()
        .map(|action| actions[*action].cost)
        .collect::<Vec<_>>();
    let largest_limit = global_limit.min(local_costs.len());
    let mut entries = Vec::new();
    let Some(maximum) = search_component(
        specification,
        actions,
        &component,
        &local_costs,
        largest_limit,
        limits,
        work,
    )?
    else {
        return Ok(None);
    };
    let smallest_relevant_limit = match maximum.status {
        SearchStatus::Optimal => insert_result(&mut entries, &component, maximum)?,
        SearchStatus::Infeasible => {
            return Ok(Some(CoverageComponentFrontier { component, entries }));
        }
        SearchStatus::Incomplete => return Ok(None),
    };
    for local_limit in 0..smallest_relevant_limit {
        let Some(result) = search_component(
            specification,
            actions,
            &component,
            &local_costs,
            local_limit,
            limits,
            work,
        )?
        else {
            return Ok(None);
        };
        match result.status {
            SearchStatus::Optimal => {
                insert_result(&mut entries, &component, result)?;
            }
            SearchStatus::Infeasible => {}
            SearchStatus::Incomplete => return Ok(None),
        }
    }
    Ok(Some(CoverageComponentFrontier { component, entries }))
}

#[allow(clippy::too_many_arguments)]
fn search_component(
    specification: &CoverageSpecification,
    actions: &[CoverageAction],
    component: &CoverageComponent,
    local_costs: &[u64],
    local_limit: usize,
    limits: CoverageSynthesisLimits,
    work: &mut CompositionWork,
) -> Result<Option<SearchResult>> {
    let remaining_calls = limits.max_oracle_calls.saturating_sub(work.oracle_calls);
    let remaining_nodes = limits.max_search_nodes.saturating_sub(work.search_nodes);
    if remaining_calls == 0 || remaining_nodes == 0 {
        return Ok(None);
    }
    let result = minimize_antitone(
        local_costs,
        local_limit,
        SearchLimits {
            oracle_calls: remaining_calls,
            search_nodes: remaining_nodes,
        },
        |local_selected| {
            let selected = local_selected
                .iter()
                .map(|local| component.actions()[*local])
                .collect::<Vec<_>>();
            Ok(!evaluate_coverage_plan_states_prevalidated(
                specification,
                actions,
                &selected,
                component.states(),
                limits.coverage,
            )?
            .criterion_holds)
        },
    )?;
    work.oracle_calls = checked_sum(
        work.oracle_calls,
        result.oracle_calls,
        "coverage composition oracle call count overflows",
    )?;
    work.search_nodes = checked_sum(
        work.search_nodes,
        result.search_nodes,
        "coverage composition search node count overflows",
    )?;
    work.cache_hits = checked_sum(
        work.cache_hits,
        result.cache_hits,
        "coverage composition cache hit count overflows",
    )?;
    Ok(Some(result))
}

fn insert_result(
    entries: &mut Vec<CoverageFrontierEntry>,
    component: &CoverageComponent,
    result: SearchResult,
) -> Result<usize> {
    let selected = result
        .selected
        .iter()
        .map(|local| component.actions()[*local])
        .collect::<Vec<_>>();
    let activations = selected.len();
    let cost = result
        .upper_bound
        .ok_or_else(|| Error::InvalidInput("optimal component result has no cost".into()))?;
    insert_nondominated(
        entries,
        CoverageFrontierEntry {
            activations,
            cost,
            selected,
        },
    );
    Ok(activations)
}

fn insert_nondominated(entries: &mut Vec<CoverageFrontierEntry>, candidate: CoverageFrontierEntry) {
    if entries.iter().any(|entry| dominates(entry, &candidate)) {
        return;
    }
    entries.retain(|entry| !dominates(&candidate, entry));
    entries.push(candidate);
    entries.sort_by(|left, right| {
        (left.activations, left.cost, &left.selected).cmp(&(
            right.activations,
            right.cost,
            &right.selected,
        ))
    });
}

fn dominates(left: &CoverageFrontierEntry, right: &CoverageFrontierEntry) -> bool {
    left.activations <= right.activations
        && left.cost <= right.cost
        && (left.activations < right.activations
            || left.cost < right.cost
            || left.selected <= right.selected)
}

fn compose(
    frontiers: &[CoverageComponentFrontier],
    max_activations: usize,
) -> Result<Option<(u64, Vec<usize>)>> {
    let mut partial = BTreeMap::from([(0usize, (0u64, Vec::new()))]);
    for frontier in frontiers {
        let mut next = BTreeMap::<usize, (u64, Vec<usize>)>::new();
        for (&used, (cost, selected)) in &partial {
            for entry in &frontier.entries {
                let activations = used.checked_add(entry.activations).ok_or_else(|| {
                    Error::InvalidInput("coverage composition activation count overflows".into())
                })?;
                if activations > max_activations {
                    continue;
                }
                let combined_cost = cost.checked_add(entry.cost).ok_or_else(|| {
                    Error::InvalidInput("coverage composition cost overflows".into())
                })?;
                let combined_selected = merge(selected, &entry.selected);
                let replace = next.get(&activations).is_none_or(|(best_cost, best)| {
                    (combined_cost, &combined_selected) < (*best_cost, best)
                });
                if replace {
                    next.insert(activations, (combined_cost, combined_selected));
                }
            }
        }
        partial = next;
        if partial.is_empty() {
            return Ok(None);
        }
    }
    Ok(partial
        .into_values()
        .min_by(|left, right| (left.0, &left.1).cmp(&(right.0, &right.1))))
}

fn merge(left: &[usize], right: &[usize]) -> Vec<usize> {
    let mut output = Vec::with_capacity(left.len() + right.len());
    let (mut i, mut j) = (0, 0);
    while i < left.len() || j < right.len() {
        if j == right.len() || (i < left.len() && left[i] < right[j]) {
            output.push(left[i]);
            i += 1;
        } else {
            output.push(right[j]);
            j += 1;
        }
    }
    output
}

fn checked_sum(left: usize, right: usize, message: &str) -> Result<usize> {
    left.checked_add(right)
        .ok_or_else(|| Error::InvalidInput(message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CoverageFence, CoverageLimits, CoverageState, PlanarCoverageModel, SparseDistanceMatrix,
    };

    fn two_state_problem() -> (CoverageSpecification, Vec<CoverageAction>) {
        let graph = SparseDistanceMatrix::from_triplets(
            6,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 3, 1.0),
                (0, 3, 1.0),
                (0, 4, 1.0),
                (1, 4, 1.0),
                (2, 4, 1.0),
                (3, 4, 1.0),
                (0, 5, 1.0),
                (1, 5, 1.0),
                (2, 5, 1.0),
                (3, 5, 1.0),
            ],
        )
        .unwrap();
        let state = CoverageState::new(0, 0, &graph, vec![0, 1, 2, 3], 1.0).unwrap();
        let specification = CoverageSpecification::new(
            6,
            PlanarCoverageModel::new(1.0, 1.0).unwrap(),
            2,
            CoverageFence::new(vec![0, 1, 2, 3]).unwrap(),
            vec![4, 5],
            0,
            vec![
                state.clone(),
                CoverageState::new(0, 1, &graph, state.base_vertices().to_vec(), 1.0).unwrap(),
            ],
            CoverageLimits::default(),
        )
        .unwrap();
        let actions = vec![
            CoverageAction::new(4, 7, vec![0]),
            CoverageAction::new(5, 3, vec![1]),
        ];
        (specification, actions)
    }

    #[test]
    fn independent_frontiers_compose_under_one_global_limit() {
        let (specification, actions) = two_state_problem();
        let infeasible = compose_coverage_frontiers(
            &specification,
            &actions,
            1,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        assert_eq!(infeasible.status(), CoverageCompositionStatus::Infeasible);

        let optimal = compose_coverage_frontiers(
            &specification,
            &actions,
            2,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        assert_eq!(optimal.status(), CoverageCompositionStatus::Optimal);
        assert_eq!(optimal.cost(), Some(10));
        assert_eq!(optimal.selected(), &[0, 1]);
        assert_eq!(optimal.frontiers().len(), 2);
    }

    #[test]
    fn a_local_work_limit_never_returns_an_optimal_claim() {
        let (specification, actions) = two_state_problem();
        let result = compose_coverage_frontiers(
            &specification,
            &actions,
            2,
            CoverageSynthesisLimits::default().with_max_oracle_calls(1),
        )
        .unwrap();
        assert_eq!(result.status(), CoverageCompositionStatus::SearchIncomplete);
        assert!(result.cost().is_none());
        assert!(result.selected().is_empty());
    }

    #[test]
    fn proof_artifacts_use_the_composed_incumbent() {
        let (specification, actions) = two_state_problem();
        let artifact = crate::CoverageSynthesisArtifact::build(
            specification,
            actions,
            2,
            CoverageSynthesisLimits::default(),
        )
        .unwrap();
        assert_eq!(artifact.status(), crate::CoverageSynthesisStatus::Optimal);
        assert_eq!(artifact.selected(), &[0, 1]);
        assert_eq!(artifact.upper_bound_cost(), Some(10));
        assert!(artifact.proof_nodes() > 0);
    }
}
