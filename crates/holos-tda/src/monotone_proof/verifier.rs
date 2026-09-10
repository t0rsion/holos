use std::collections::BTreeSet;

use super::helpers::{
    add_terms, blocker_bound, check_branch_count, check_survival, difference, insert_sorted, merge,
    selected_cost, start_node,
};
use super::{BoundKind, ProofLimits, ProofNode, ProofWork};
use crate::{Error, Result};

pub(super) struct Verifier<'a, F> {
    pub(super) costs: &'a [u64],
    pub(super) max_selected: usize,
    pub(super) cutoff: Option<u64>,
    pub(super) limits: ProofLimits,
    pub(super) oracle: &'a mut F,
    pub(super) work: ProofWork,
}

impl<F> Verifier<'_, F>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    pub(super) fn verify_node(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<()> {
        start_node(&mut self.work, self.limits, depth)?;
        let included_cost = selected_cost(self.costs, &included)?;
        match proof {
            ProofNode::Cost => self.verify_cost_leaf(included_cost),
            ProofNode::SurvivingMaximum => self.verify_maximum_leaf(&included, &available),
            ProofNode::SurvivingSelectionLimit => self.verify_selection_leaf(&included),
            ProofNode::BlockerBound { kind, blockers } => {
                self.verify_blocker_leaf(kind, blockers, &included, &available, included_cost)
            }
            ProofNode::Branch { blocker, children } => {
                self.verify_branch(blocker, children, included, available, depth)
            }
        }
    }

    fn verify_cost_leaf(&self, included_cost: u64) -> Result<()> {
        if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
            return Err(Error::InvalidInput(
                "monotone cost leaf does not reach the incumbent".into(),
            ));
        }
        Ok(())
    }

    fn verify_maximum_leaf(&mut self, included: &[usize], available: &[usize]) -> Result<()> {
        if !check_survival(
            &mut self.work,
            self.limits,
            self.oracle,
            &merge(included, available),
        )? {
            return Err(Error::InvalidInput(
                "monotone maximal-survival leaf is feasible".into(),
            ));
        }
        Ok(())
    }

    fn verify_selection_leaf(&mut self, included: &[usize]) -> Result<()> {
        if included.len() != self.max_selected
            || !check_survival(&mut self.work, self.limits, self.oracle, included)?
        {
            return Err(Error::InvalidInput(
                "monotone selection-limit leaf is invalid".into(),
            ));
        }
        Ok(())
    }

    fn verify_blocker_leaf(
        &mut self,
        kind: &BoundKind,
        blockers: &[Vec<usize>],
        included: &[usize],
        available: &[usize],
        included_cost: u64,
    ) -> Result<()> {
        self.verify_blockers(included, available, blockers)?;
        if self.blocker_leaf_closes(kind, blockers, included.len(), included_cost) {
            return Ok(());
        }
        Err(Error::InvalidInput(
            "monotone blocker leaf does not close its branch".into(),
        ))
    }

    fn blocker_leaf_closes(
        &self,
        kind: &BoundKind,
        blockers: &[Vec<usize>],
        included_count: usize,
        included_cost: u64,
    ) -> bool {
        match kind {
            BoundKind::Selections => {
                included_count.saturating_add(blockers.len()) > self.max_selected
            }
            BoundKind::Cost => self.cutoff.is_some_and(|cutoff| {
                included_cost
                    .checked_add(blocker_bound(self.costs, blockers).unwrap_or(u64::MAX))
                    .is_some_and(|bound| bound >= cutoff)
            }),
        }
    }

    fn verify_branch(
        &mut self,
        blocker: &[usize],
        children: &[ProofNode],
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<()> {
        let blocker_family = [blocker.to_vec()];
        self.verify_blockers(&included, &available, &blocker_family)?;
        check_branch_count(blocker.len(), children.len())?;
        let mut excluded = BTreeSet::new();
        for (&candidate, child) in blocker.iter().zip(children) {
            let mut child_included = included.clone();
            insert_sorted(&mut child_included, candidate);
            let child_available = available
                .iter()
                .copied()
                .filter(|item| *item != candidate && !excluded.contains(item))
                .collect();
            self.verify_node(child, child_included, child_available, depth + 1)?;
            excluded.insert(candidate);
        }
        Ok(())
    }

    fn verify_blockers(
        &mut self,
        included: &[usize],
        available: &[usize],
        blockers: &[Vec<usize>],
    ) -> Result<()> {
        let mut used = BTreeSet::new();
        for blocker in blockers {
            if blocker.is_empty()
                || blocker.iter().any(|candidate| {
                    available.binary_search(candidate).is_err() || !used.insert(*candidate)
                })
                || blocker.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(Error::InvalidInput(
                    "monotone blocker family is not canonical and disjoint".into(),
                ));
            }
            let witness = merge(included, &difference(available, blocker));
            if !check_survival(&mut self.work, self.limits, self.oracle, &witness)? {
                return Err(Error::InvalidInput(
                    "monotone blocker complement does not survive".into(),
                ));
            }
        }
        add_terms(&mut self.work, self.limits, blockers)?;
        Ok(())
    }
}
