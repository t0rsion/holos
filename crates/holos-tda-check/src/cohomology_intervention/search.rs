use std::collections::{BTreeMap, BTreeSet};

use crate::ProofError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SearchStatus {
    Optimal,
    Infeasible,
    Incomplete,
}

pub(super) struct SearchResult {
    pub(super) status: SearchStatus,
    pub(super) selected: Vec<usize>,
    pub(super) lower_bound: Option<u64>,
    pub(super) upper_bound: Option<u64>,
    pub(super) oracle_calls: usize,
    pub(super) search_nodes: usize,
    pub(super) cache_hits: usize,
    pub(super) root_blockers: Vec<Vec<usize>>,
    pub(super) root_blocker_bound: u64,
}

pub(super) struct Search<'a, F> {
    costs: &'a [u64],
    max_selected: usize,
    oracle_limit: usize,
    node_limit: usize,
    oracle: F,
    cache: BTreeMap<Vec<usize>, bool>,
    oracle_calls: usize,
    cache_hits: usize,
    search_nodes: usize,
    best: Option<(u64, Vec<usize>)>,
}

struct BlockerPacking {
    blockers: Vec<Vec<usize>>,
    complete: bool,
}

enum ExploreEntry {
    Done(bool),
    Continue(u64),
}

enum BranchPlan {
    Done(bool),
    Branch(Vec<usize>),
}

enum GreedyPlan {
    Done(bool),
    Minimize(Vec<usize>),
}

impl<'a, F> Search<'a, F>
where
    F: FnMut(&[usize]) -> Result<bool, ProofError>,
{
    pub(super) fn new(
        costs: &'a [u64],
        max_selected: usize,
        oracle_limit: usize,
        node_limit: usize,
        oracle: F,
    ) -> Self {
        Self {
            costs,
            max_selected: max_selected.min(costs.len()),
            oracle_limit,
            node_limit,
            oracle,
            cache: BTreeMap::new(),
            oracle_calls: 0,
            cache_hits: 0,
            search_nodes: 0,
            best: None,
        }
    }

    pub(super) fn run(&mut self) -> Result<SearchResult, ProofError> {
        if let Some(result) = self.initial_result()? {
            return Ok(result);
        }
        let all = (0..self.costs.len()).collect::<Vec<_>>();
        let root_packing = self.pack_blockers(&[], &all)?;
        let root_bound = blocker_bound(self.costs, &root_packing.blockers)?;
        if root_packing.complete {
            let _ = self.greedy_upper(&all)?;
        }
        let complete = root_packing.complete && self.explore(Vec::new(), all)?;
        self.final_result(complete, root_bound, root_packing.blockers)
    }

    fn initial_result(&mut self) -> Result<Option<SearchResult>, ProofError> {
        let empty = Vec::new();
        let Some(empty_survives) = self.evaluate(&empty)? else {
            return self
                .result(SearchStatus::Incomplete, vec![], Some(0), None, vec![])
                .map(Some);
        };
        if !empty_survives {
            return self
                .result(SearchStatus::Optimal, vec![], Some(0), Some(0), vec![])
                .map(Some);
        }
        let all = (0..self.costs.len()).collect::<Vec<_>>();
        let Some(all_survives) = self.evaluate(&all)? else {
            return self
                .result(SearchStatus::Incomplete, vec![], Some(0), None, vec![])
                .map(Some);
        };
        if all_survives {
            return self
                .result(SearchStatus::Infeasible, vec![], None, None, vec![])
                .map(Some);
        }
        Ok(None)
    }

    fn final_result(
        &self,
        complete: bool,
        root_bound: u64,
        root_blockers: Vec<Vec<usize>>,
    ) -> Result<SearchResult, ProofError> {
        let (status, selected, lower, upper) = match &self.best {
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
            None if complete => (SearchStatus::Infeasible, vec![], None, None),
            None => (SearchStatus::Incomplete, vec![], Some(root_bound), None),
        };
        self.result(status, selected, lower, upper, root_blockers)
    }

    fn result(
        &self,
        status: SearchStatus,
        selected: Vec<usize>,
        lower_bound: Option<u64>,
        upper_bound: Option<u64>,
        root_blockers: Vec<Vec<usize>>,
    ) -> Result<SearchResult, ProofError> {
        Ok(SearchResult {
            status,
            selected,
            lower_bound,
            upper_bound,
            oracle_calls: self.oracle_calls,
            search_nodes: self.search_nodes,
            cache_hits: self.cache_hits,
            root_blocker_bound: blocker_bound(self.costs, &root_blockers)?,
            root_blockers,
        })
    }

    fn evaluate(&mut self, selected: &[usize]) -> Result<Option<bool>, ProofError> {
        if let Some(value) = self.cache.get(selected) {
            self.cache_hits = self.cache_hits.saturating_add(1);
            return Ok(Some(*value));
        }
        if self.oracle_calls == self.oracle_limit {
            return Ok(None);
        }
        let value = (self.oracle)(selected)?;
        self.oracle_calls += 1;
        self.cache.insert(selected.to_vec(), value);
        Ok(Some(value))
    }

    fn greedy_upper(&mut self, available: &[usize]) -> Result<bool, ProofError> {
        let mut selected = match self.build_greedy_selection(available)? {
            GreedyPlan::Done(complete) => return Ok(complete),
            GreedyPlan::Minimize(selected) => selected,
        };
        if self.evaluate(&selected)?.is_none() {
            return Ok(false);
        }
        if !self.minimize_greedy_selection(&mut selected)? {
            return Ok(false);
        }
        self.update_best(selected)?;
        Ok(true)
    }

    fn build_greedy_selection(&mut self, available: &[usize]) -> Result<GreedyPlan, ProofError> {
        let mut selected = Vec::new();
        while self.evaluate(&selected)?.is_some_and(|survives| survives) {
            if selected.len() == self.max_selected {
                return Ok(GreedyPlan::Done(true));
            }
            let remaining = difference(available, &selected);
            let packing = self.pack_blockers(&selected, &remaining)?;
            let Some(blocker) = packing.blockers.first() else {
                return Ok(GreedyPlan::Done(packing.complete));
            };
            let candidate = cheapest_candidate(self.costs, blocker);
            insert_sorted(&mut selected, candidate);
            if !packing.complete {
                return Ok(GreedyPlan::Done(false));
            }
        }
        Ok(GreedyPlan::Minimize(selected))
    }

    fn minimize_greedy_selection(&mut self, selected: &mut Vec<usize>) -> Result<bool, ProofError> {
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

    fn explore(&mut self, included: Vec<usize>, available: Vec<usize>) -> Result<bool, ProofError> {
        let included_cost = match self.enter_node(&included)? {
            ExploreEntry::Done(complete) => return Ok(complete),
            ExploreEntry::Continue(cost) => cost,
        };
        match self.plan_branch(&included, &available, included_cost)? {
            BranchPlan::Done(complete) => Ok(complete),
            BranchPlan::Branch(branch) => self.explore_branch(&included, &available, branch),
        }
    }

    fn plan_branch(
        &mut self,
        included: &[usize],
        available: &[usize],
        included_cost: u64,
    ) -> Result<BranchPlan, ProofError> {
        let union = merge(included, available);
        let Some(union_survives) = self.evaluate(&union)? else {
            return Ok(BranchPlan::Done(false));
        };
        if union_survives {
            return Ok(BranchPlan::Done(true));
        }
        let packing = self.pack_blockers(included, available)?;
        let bound = included_cost
            .checked_add(blocker_bound(self.costs, &packing.blockers)?)
            .ok_or_else(|| ProofError::new("intervention search bound overflows"))?;
        if self.branch_is_closed(included.len(), packing.blockers.len(), bound) {
            return Ok(BranchPlan::Done(true));
        }
        if !packing.complete {
            return Ok(BranchPlan::Done(false));
        }
        let Some(branch) = self.branch_candidates(packing.blockers) else {
            return Ok(BranchPlan::Done(true));
        };
        Ok(BranchPlan::Branch(branch))
    }

    fn enter_node(&mut self, included: &[usize]) -> Result<ExploreEntry, ProofError> {
        if self.search_nodes == self.node_limit {
            return Ok(ExploreEntry::Done(false));
        }
        self.search_nodes += 1;
        let Some(survives) = self.evaluate(included)? else {
            return Ok(ExploreEntry::Done(false));
        };
        if !survives {
            self.update_best(included.to_vec())?;
            return Ok(ExploreEntry::Done(true));
        }
        if included.len() == self.max_selected {
            return Ok(ExploreEntry::Done(true));
        }
        let included_cost = selected_cost(self.costs, included)?;
        if self
            .best
            .as_ref()
            .is_some_and(|(best, _)| included_cost >= *best)
        {
            Ok(ExploreEntry::Done(true))
        } else {
            Ok(ExploreEntry::Continue(included_cost))
        }
    }

    fn branch_is_closed(&self, included: usize, blockers: usize, bound: u64) -> bool {
        included.saturating_add(blockers) > self.max_selected
            || self.best.as_ref().is_some_and(|(best, _)| bound >= *best)
    }

    fn branch_candidates(&self, blockers: Vec<Vec<usize>>) -> Option<Vec<usize>> {
        let mut branch = blockers
            .into_iter()
            .min_by_key(|blocker| (blocker.len(), blocker_min_cost(self.costs, blocker)))?;
        branch.sort_by_key(|candidate| (self.costs[*candidate], *candidate));
        Some(branch)
    }

    fn explore_branch(
        &mut self,
        included: &[usize],
        available: &[usize],
        branch: Vec<usize>,
    ) -> Result<bool, ProofError> {
        let mut excluded = BTreeSet::new();
        for candidate in branch {
            let mut child_included = included.to_vec();
            insert_sorted(&mut child_included, candidate);
            let child_available = available
                .iter()
                .copied()
                .filter(|item| *item != candidate && !excluded.contains(item))
                .collect();
            if !self.explore(child_included, child_available)? {
                return Ok(false);
            }
            excluded.insert(candidate);
        }
        Ok(true)
    }

    fn pack_blockers(
        &mut self,
        included: &[usize],
        available: &[usize],
    ) -> Result<BlockerPacking, ProofError> {
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

    fn update_best(&mut self, selected: Vec<usize>) -> Result<(), ProofError> {
        let cost = selected_cost(self.costs, &selected)?;
        if self.best.as_ref().is_none_or(|(best_cost, best)| {
            cost < *best_cost || (cost == *best_cost && selected < *best)
        }) {
            self.best = Some((cost, selected));
        }
        Ok(())
    }
}

fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64, ProofError> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| ProofError::new("intervention selected cost overflows"))
    })
}

fn blocker_min_cost(costs: &[u64], blocker: &[usize]) -> u64 {
    blocker
        .iter()
        .map(|candidate| costs[*candidate])
        .min()
        .unwrap_or(0)
}

fn cheapest_candidate(costs: &[u64], blocker: &[usize]) -> usize {
    blocker
        .iter()
        .copied()
        .min_by_key(|candidate| (costs[*candidate], *candidate))
        .expect("a blocker is nonempty")
}

fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64, ProofError> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| ProofError::new("intervention blocker bound overflows"))
    })
}

fn insert_sorted(values: &mut Vec<usize>, value: usize) {
    if let Err(position) = values.binary_search(&value) {
        values.insert(position, value);
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
