//! Exact weighted search for an antitone survival predicate.

use std::collections::{BTreeMap, BTreeSet};

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SearchStatus {
    Optimal,
    Infeasible,
    Incomplete,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SearchLimits {
    pub oracle_calls: usize,
    pub search_nodes: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchResult {
    pub status: SearchStatus,
    pub selected: Vec<usize>,
    pub lower_bound: Option<u64>,
    pub upper_bound: Option<u64>,
    pub oracle_calls: usize,
    pub search_nodes: usize,
    pub cache_hits: usize,
    pub root_blockers: Vec<Vec<usize>>,
    pub root_blocker_bound: u64,
}

pub(crate) fn minimize_antitone<F>(
    costs: &[u64],
    max_selected: usize,
    limits: SearchLimits,
    oracle: F,
) -> Result<SearchResult>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    if costs.contains(&0) {
        return Err(Error::InvalidInput(
            "monotone search candidate costs must be positive".into(),
        ));
    }
    costs.iter().try_fold(0u64, |sum, cost| {
        sum.checked_add(*cost)
            .ok_or_else(|| Error::InvalidInput("monotone search cost sum overflows".into()))
    })?;
    if limits.oracle_calls == 0 || limits.search_nodes == 0 {
        return Err(Error::InvalidInput(
            "monotone search limits must be positive".into(),
        ));
    }
    let mut search = Search {
        costs,
        max_selected: max_selected.min(costs.len()),
        limits,
        oracle,
        cache: BTreeMap::new(),
        oracle_calls: 0,
        cache_hits: 0,
        search_nodes: 0,
        best: None,
    };
    search.run()
}

struct Search<'a, F> {
    costs: &'a [u64],
    max_selected: usize,
    limits: SearchLimits,
    oracle: F,
    cache: BTreeMap<Vec<usize>, bool>,
    oracle_calls: usize,
    cache_hits: usize,
    search_nodes: usize,
    best: Option<(u64, Vec<usize>)>,
}

#[derive(Debug, Default)]
struct BlockerPacking {
    blockers: Vec<Vec<usize>>,
    complete: bool,
}

enum ExploreStart {
    Return(bool),
    Continue(u64),
}

impl<F> Search<'_, F>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    fn run(&mut self) -> Result<SearchResult> {
        let empty = Vec::new();
        let all = (0..self.costs.len()).collect::<Vec<_>>();
        if let Some(result) = self.extreme_result(&empty, &all)? {
            return Ok(result);
        }
        let root_packing = self.pack_blockers(&empty, &all)?;
        let root_bound = blocker_bound(self.costs, &root_packing.blockers)?;
        let complete = self.search_root(&all, root_packing.complete)?;
        Ok(self.finish_result(root_packing, root_bound, complete))
    }

    fn extreme_result(&mut self, empty: &[usize], all: &[usize]) -> Result<Option<SearchResult>> {
        let Some(empty_survives) = self.evaluate(empty)? else {
            return Ok(Some(self.incomplete_empty()));
        };
        if !empty_survives {
            return Ok(Some(self.result(
                SearchStatus::Optimal,
                Vec::new(),
                Some(0),
                Some(0),
                vec![],
            )));
        }
        let Some(all_survives) = self.evaluate(all)? else {
            return Ok(Some(self.incomplete_empty()));
        };
        if all_survives {
            return Ok(Some(self.result(
                SearchStatus::Infeasible,
                Vec::new(),
                None,
                None,
                vec![],
            )));
        }
        Ok(None)
    }

    fn incomplete_empty(&self) -> SearchResult {
        self.result(SearchStatus::Incomplete, Vec::new(), Some(0), None, vec![])
    }

    fn search_root(&mut self, all: &[usize], packing_complete: bool) -> Result<bool> {
        if !packing_complete {
            return Ok(false);
        }
        let _ = self.greedy_upper(all)?;
        self.explore(Vec::new(), all.to_vec())
    }

    fn finish_result(
        &self,
        root_packing: BlockerPacking,
        root_bound: u64,
        complete: bool,
    ) -> SearchResult {
        let (status, selected, lower_bound, upper_bound) = match &self.best {
            Some((cost, selected)) if complete || root_bound == *cost => (
                SearchStatus::Optimal,
                selected.clone(),
                Some(*cost),
                Some(*cost),
            ),
            Some((cost, selected)) => (
                SearchStatus::Incomplete,
                selected.clone(),
                Some(root_bound),
                Some(*cost),
            ),
            None if complete => (SearchStatus::Infeasible, Vec::new(), None, None),
            None => (SearchStatus::Incomplete, Vec::new(), Some(root_bound), None),
        };
        self.result(
            status,
            selected,
            lower_bound,
            upper_bound,
            root_packing.blockers,
        )
    }

    fn result(
        &self,
        status: SearchStatus,
        selected: Vec<usize>,
        lower_bound: Option<u64>,
        upper_bound: Option<u64>,
        root_blockers: Vec<Vec<usize>>,
    ) -> SearchResult {
        let root_blocker_bound = blocker_bound(self.costs, &root_blockers).unwrap_or(0);
        SearchResult {
            status,
            selected,
            lower_bound,
            upper_bound,
            oracle_calls: self.oracle_calls,
            search_nodes: self.search_nodes,
            cache_hits: self.cache_hits,
            root_blockers,
            root_blocker_bound,
        }
    }

    fn evaluate(&mut self, selected: &[usize]) -> Result<Option<bool>> {
        if let Some(value) = self.cache.get(selected) {
            self.cache_hits = self.cache_hits.saturating_add(1);
            return Ok(Some(*value));
        }
        if self.oracle_calls == self.limits.oracle_calls {
            return Ok(None);
        }
        let value = (self.oracle)(selected)?;
        self.oracle_calls += 1;
        self.cache.insert(selected.to_vec(), value);
        Ok(Some(value))
    }

    fn greedy_upper(&mut self, available: &[usize]) -> Result<bool> {
        let Some(mut selected) = self.grow_greedy(available)? else {
            return Ok(false);
        };
        let Some(survives) = self.evaluate(&selected)? else {
            return Ok(false);
        };
        if survives {
            return Ok(true);
        }
        if !self.minimize_greedy(&mut selected)? {
            return Ok(false);
        }
        self.update_best(selected)?;
        Ok(true)
    }

    fn grow_greedy(&mut self, available: &[usize]) -> Result<Option<Vec<usize>>> {
        let mut selected = Vec::new();
        loop {
            let Some(survives) = self.evaluate(&selected)? else {
                return Ok(None);
            };
            if !survives {
                return Ok(Some(selected));
            }
            if selected.len() == self.max_selected {
                return Ok(Some(selected));
            }
            let remaining = difference(available, &selected);
            let packing = self.pack_blockers(&selected, &remaining)?;
            let Some(blocker) = packing.blockers.first() else {
                return Ok(packing.complete.then_some(selected));
            };
            let candidate = blocker
                .iter()
                .copied()
                .min_by_key(|candidate| (self.costs[*candidate], *candidate))
                .expect("a blocker is nonempty");
            insert_sorted(&mut selected, candidate);
            if !packing.complete {
                return Ok(None);
            }
        }
    }

    fn minimize_greedy(&mut self, selected: &mut Vec<usize>) -> Result<bool> {
        for candidate in selected.clone().into_iter().rev() {
            let reduced = without(selected, candidate);
            let Some(survives) = self.evaluate(&reduced)? else {
                return Ok(false);
            };
            if !survives {
                *selected = reduced;
            }
        }
        Ok(true)
    }

    fn explore(&mut self, included: Vec<usize>, available: Vec<usize>) -> Result<bool> {
        let included_cost = match self.start_explore(&included, &available)? {
            ExploreStart::Return(complete) => return Ok(complete),
            ExploreStart::Continue(cost) => cost,
        };
        let packing = self.pack_blockers(&included, &available)?;
        if let Some(complete) = self.packing_result(&included, included_cost, &packing)? {
            return Ok(complete);
        }
        let Some(branch) = select_branch(packing.blockers, self.costs) else {
            return Ok(true);
        };
        self.explore_branch(included, available, branch)
    }

    fn start_explore(&mut self, included: &[usize], available: &[usize]) -> Result<ExploreStart> {
        if self.search_nodes == self.limits.search_nodes {
            return Ok(ExploreStart::Return(false));
        }
        self.search_nodes += 1;
        let Some(survives) = self.evaluate(included)? else {
            return Ok(ExploreStart::Return(false));
        };
        if !survives {
            self.update_best(included.to_vec())?;
            return Ok(ExploreStart::Return(true));
        }
        if included.len() == self.max_selected {
            return Ok(ExploreStart::Return(true));
        }
        let included_cost = selected_cost(self.costs, included)?;
        if self
            .best
            .as_ref()
            .is_some_and(|(best, _)| included_cost >= *best)
        {
            return Ok(ExploreStart::Return(true));
        }
        let union = merge(included, available);
        let Some(union_survives) = self.evaluate(&union)? else {
            return Ok(ExploreStart::Return(false));
        };
        if union_survives {
            return Ok(ExploreStart::Return(true));
        }
        Ok(ExploreStart::Continue(included_cost))
    }

    fn packing_result(
        &self,
        included: &[usize],
        included_cost: u64,
        packing: &BlockerPacking,
    ) -> Result<Option<bool>> {
        let bound = included_cost
            .checked_add(blocker_bound(self.costs, &packing.blockers)?)
            .ok_or_else(|| Error::InvalidInput("monotone search bound overflows".into()))?;
        let cardinality_bound = included.len().saturating_add(packing.blockers.len());
        if cardinality_bound > self.max_selected
            || self.best.as_ref().is_some_and(|(best, _)| bound >= *best)
        {
            return Ok(Some(true));
        }
        if !packing.complete {
            return Ok(Some(false));
        }
        Ok(None)
    }

    fn explore_branch(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        branch: Vec<usize>,
    ) -> Result<bool> {
        let mut excluded = BTreeSet::new();
        let mut complete = true;
        for candidate in branch {
            let mut child_included = included.clone();
            insert_sorted(&mut child_included, candidate);
            let child_available = available
                .iter()
                .copied()
                .filter(|item| *item != candidate && !excluded.contains(item))
                .collect();
            if !self.explore(child_included, child_available)? {
                complete = false;
                break;
            }
            excluded.insert(candidate);
        }
        Ok(complete)
    }

    fn pack_blockers(&mut self, included: &[usize], available: &[usize]) -> Result<BlockerPacking> {
        let mut packing = BlockerPacking {
            blockers: Vec::new(),
            complete: true,
        };
        let mut used = Vec::new();
        loop {
            let base = merge(included, &used);
            let Some(base_survives) = self.evaluate(&base)? else {
                packing.complete = false;
                return Ok(packing);
            };
            if !base_survives {
                return Ok(packing);
            }
            let mut retained = used.clone();
            for candidate in available
                .iter()
                .copied()
                .filter(|candidate| used.binary_search(candidate).is_err())
            {
                let mut trial = merge(included, &retained);
                insert_sorted(&mut trial, candidate);
                let Some(survives) = self.evaluate(&trial)? else {
                    packing.complete = false;
                    return Ok(packing);
                };
                if survives {
                    insert_sorted(&mut retained, candidate);
                }
            }
            let blocker = difference(available, &retained);
            if blocker.is_empty() {
                return Ok(packing);
            }
            for candidate in &blocker {
                insert_sorted(&mut used, *candidate);
            }
            packing.blockers.push(blocker);
        }
    }

    fn update_best(&mut self, selected: Vec<usize>) -> Result<()> {
        let cost = selected_cost(self.costs, &selected)?;
        if self.best.as_ref().is_none_or(|(best_cost, best)| {
            cost < *best_cost || (cost == *best_cost && selected < *best)
        }) {
            self.best = Some((cost, selected));
        }
        Ok(())
    }
}

fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| Error::InvalidInput("monotone search selected cost overflows".into()))
    })
}

fn blocker_min_cost(costs: &[u64], blocker: &[usize]) -> u64 {
    blocker
        .iter()
        .map(|candidate| costs[*candidate])
        .min()
        .unwrap_or(0)
}

fn select_branch(mut blockers: Vec<Vec<usize>>, costs: &[u64]) -> Option<Vec<usize>> {
    let mut branch = blockers
        .drain(..)
        .min_by_key(|blocker| (blocker.len(), blocker_min_cost(costs, blocker)))?;
    branch.sort_by_key(|candidate| (costs[*candidate], *candidate));
    Some(branch)
}

fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| Error::InvalidInput("monotone search blocker bound overflows".into()))
    })
}

fn insert_sorted(values: &mut Vec<usize>, value: usize) {
    match values.binary_search(&value) {
        Ok(_) => {}
        Err(position) => values.insert(position, value),
    }
}

fn without(values: &[usize], removed: usize) -> Vec<usize> {
    values
        .iter()
        .copied()
        .filter(|value| *value != removed)
        .collect()
}

fn difference(values: &[usize], removed: &[usize]) -> Vec<usize> {
    values
        .iter()
        .copied()
        .filter(|value| removed.binary_search(value).is_err())
        .collect()
}

fn merge(left: &[usize], right: &[usize]) -> Vec<usize> {
    left.iter()
        .chain(right)
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hitting_oracle(groups: Vec<Vec<usize>>) -> impl FnMut(&[usize]) -> Result<bool> {
        move |selected| {
            Ok(groups.iter().any(|group| {
                group
                    .iter()
                    .all(|candidate| selected.binary_search(candidate).is_err())
            }))
        }
    }

    fn exhaustive(costs: &[u64], max_selected: usize, groups: &[Vec<usize>]) -> Option<u64> {
        (0usize..1usize << costs.len())
            .filter(|mask| mask.count_ones() as usize <= max_selected)
            .filter(|mask| {
                groups
                    .iter()
                    .all(|group| group.iter().any(|candidate| mask & (1 << candidate) != 0))
            })
            .map(|mask| {
                costs
                    .iter()
                    .enumerate()
                    .filter_map(|(candidate, cost)| (mask & (1 << candidate) != 0).then_some(cost))
                    .sum()
            })
            .min()
    }

    #[test]
    fn weighted_blocker_search_matches_exhaustive_hitting_sets() {
        let limits = SearchLimits {
            oracle_calls: 100_000,
            search_nodes: 100_000,
        };
        for seed in 0..64usize {
            let costs = (0..8)
                .map(|candidate| 1 + ((candidate * 7 + seed * 3) % 11) as u64)
                .collect::<Vec<_>>();
            let groups = (0..4)
                .map(|group| {
                    (0..8)
                        .filter(|candidate| (candidate * 5 + group * 3 + seed) % 7 < 3)
                        .collect::<Vec<_>>()
                })
                .filter(|group| !group.is_empty())
                .collect::<Vec<_>>();
            let expected = exhaustive(&costs, 5, &groups);
            let actual =
                minimize_antitone(&costs, 5, limits, hitting_oracle(groups.clone())).unwrap();
            assert_eq!(actual.upper_bound, expected, "seed {seed}");
            assert_eq!(
                actual.status,
                if expected.is_some() {
                    SearchStatus::Optimal
                } else {
                    SearchStatus::Infeasible
                }
            );
        }
    }

    #[test]
    fn necessary_set_prunes_a_wide_candidate_family() {
        let mut groups = vec![vec![0]];
        groups.push((1..128).collect());
        let result = minimize_antitone(
            &vec![1; 128],
            2,
            SearchLimits {
                oracle_calls: 2_000,
                search_nodes: 2_000,
            },
            hitting_oracle(groups),
        )
        .unwrap();
        assert_eq!(result.status, SearchStatus::Optimal);
        assert_eq!(result.upper_bound, Some(2));
        assert!(result.oracle_calls < 1_000);
        assert_eq!(result.root_blocker_bound, 2);
    }

    #[test]
    fn work_limit_keeps_a_checked_bound_and_incumbent() {
        let result = minimize_antitone(
            &[4, 2, 7, 1, 9, 3],
            3,
            SearchLimits {
                oracle_calls: 12,
                search_nodes: 2,
            },
            hitting_oracle(vec![vec![0, 1], vec![2, 3], vec![4, 5]]),
        )
        .unwrap();
        assert!(matches!(
            result.status,
            SearchStatus::Incomplete | SearchStatus::Optimal
        ));
        if let (Some(lower), Some(upper)) = (result.lower_bound, result.upper_bound) {
            assert!(lower <= upper);
        }
    }
}
