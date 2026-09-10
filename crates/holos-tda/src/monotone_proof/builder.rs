use std::collections::BTreeSet;

use super::helpers::{
    add_terms, blocker_bound, blocker_min_cost, check_survival, difference, insert_sorted, merge,
    selected_cost, start_node,
};
use super::{BoundKind, ProofLimits, ProofNode, ProofWork};
use crate::{Error, Result};

pub(super) struct Builder<'a, F> {
    pub(super) costs: &'a [u64],
    pub(super) max_selected: usize,
    pub(super) cutoff: Option<u64>,
    pub(super) limits: ProofLimits,
    pub(super) oracle: &'a mut F,
    pub(super) work: ProofWork,
}

impl<F> Builder<'_, F>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    pub(super) fn prove(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<ProofNode> {
        start_node(&mut self.work, self.limits, depth)?;
        let included_cost = selected_cost(self.costs, &included)?;
        if let Some(leaf) = self.early_leaf(&included, &available, included_cost)? {
            return Ok(leaf);
        }
        self.prove_with_blockers(included, available, included_cost, depth)
    }

    fn early_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
        included_cost: u64,
    ) -> Result<Option<ProofNode>> {
        if self.cutoff.is_some_and(|cutoff| included_cost >= cutoff) {
            return Ok(Some(ProofNode::Cost));
        }
        if included.len() == self.max_selected {
            self.check_selection_limit(included)?;
            return Ok(Some(ProofNode::SurvivingSelectionLimit));
        }
        if check_survival(
            &mut self.work,
            self.limits,
            self.oracle,
            &merge(included, available),
        )? {
            return Ok(Some(ProofNode::SurvivingMaximum));
        }
        Ok(None)
    }

    fn check_selection_limit(&mut self, included: &[usize]) -> Result<()> {
        if !check_survival(&mut self.work, self.limits, self.oracle, included)? {
            return Err(Error::InvalidInput(
                "monotone proof found a cheaper feasible selection".into(),
            ));
        }
        Ok(())
    }

    fn prove_with_blockers(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        included_cost: u64,
        depth: usize,
    ) -> Result<ProofNode> {
        let blockers = self.pack_blockers(&included, &available)?;
        if blockers.is_empty() {
            return Err(Error::InvalidInput(
                "monotone proof found an unreported feasible selection".into(),
            ));
        }
        if included.len().saturating_add(blockers.len()) > self.max_selected {
            return self.blocker_leaf(BoundKind::Selections, blockers);
        }
        let bound = self.blocked_cost(included_cost, &blockers)?;
        if self.cutoff.is_some_and(|cutoff| bound >= cutoff) {
            return self.blocker_leaf(BoundKind::Cost, blockers);
        }
        let blocker = blockers
            .into_iter()
            .min_by_key(|blocker| (blocker.len(), blocker_min_cost(self.costs, blocker)))
            .expect("a nonempty packing has a blocker");
        add_terms(&mut self.work, self.limits, std::slice::from_ref(&blocker))?;
        self.prove_branch(included, available, blocker, depth)
    }

    fn blocker_leaf(&mut self, kind: BoundKind, blockers: Vec<Vec<usize>>) -> Result<ProofNode> {
        add_terms(&mut self.work, self.limits, &blockers)?;
        Ok(ProofNode::BlockerBound { kind, blockers })
    }

    fn blocked_cost(&self, included_cost: u64, blockers: &[Vec<usize>]) -> Result<u64> {
        included_cost
            .checked_add(blocker_bound(self.costs, blockers)?)
            .ok_or_else(|| Error::InvalidInput("monotone proof cost bound overflows".into()))
    }

    fn prove_branch(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        blocker: Vec<usize>,
        depth: usize,
    ) -> Result<ProofNode> {
        let mut children = Vec::with_capacity(blocker.len());
        let mut excluded = BTreeSet::new();
        for &candidate in &blocker {
            let mut child_included = included.clone();
            insert_sorted(&mut child_included, candidate);
            let child_available = available
                .iter()
                .copied()
                .filter(|item| *item != candidate && !excluded.contains(item))
                .collect();
            children.push(self.prove(child_included, child_available, depth + 1)?);
            excluded.insert(candidate);
        }
        Ok(ProofNode::Branch { blocker, children })
    }

    fn pack_blockers(
        &mut self,
        included: &[usize],
        available: &[usize],
    ) -> Result<Vec<Vec<usize>>> {
        let mut blockers = Vec::new();
        let mut used = Vec::new();
        while check_survival(
            &mut self.work,
            self.limits,
            self.oracle,
            &merge(included, &used),
        )? {
            let mut retained = used.clone();
            for candidate in available
                .iter()
                .copied()
                .filter(|candidate| used.binary_search(candidate).is_err())
            {
                let mut trial = merge(included, &retained);
                insert_sorted(&mut trial, candidate);
                if check_survival(&mut self.work, self.limits, self.oracle, &trial)? {
                    insert_sorted(&mut retained, candidate);
                }
            }
            let blocker = difference(available, &retained);
            if blocker.is_empty() {
                break;
            }
            for &candidate in &blocker {
                insert_sorted(&mut used, candidate);
            }
            blockers.push(blocker);
        }
        Ok(blockers)
    }
}
