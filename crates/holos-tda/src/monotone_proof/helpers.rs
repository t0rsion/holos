use super::ProofNode;
use super::{ProofLimits, ProofWork};
use crate::{Error, Result};

pub(super) use crate::monotone_search::{blocker_min_cost, difference, insert_sorted, merge};

pub(super) fn start_node(work: &mut ProofWork, limits: ProofLimits, depth: usize) -> Result<()> {
    work.nodes = work
        .nodes
        .checked_add(1)
        .ok_or_else(|| Error::InvalidInput("monotone proof node count overflows".into()))?;
    if work.nodes > limits.nodes || depth > limits.depth {
        return Err(Error::InvalidInput(
            "monotone proof exceeds its node or depth limit".into(),
        ));
    }
    Ok(())
}

pub(super) fn check_survival<F>(
    work: &mut ProofWork,
    limits: ProofLimits,
    oracle: &mut F,
    selected: &[usize],
) -> Result<bool>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    work.checks = work
        .checks
        .checked_add(1)
        .ok_or_else(|| Error::InvalidInput("monotone proof check count overflows".into()))?;
    if work.checks > limits.checks {
        return Err(Error::InvalidInput(
            "monotone proof checks exceed their limit".into(),
        ));
    }
    oracle(selected)
}

pub(super) fn add_terms(
    work: &mut ProofWork,
    limits: ProofLimits,
    blockers: &[Vec<usize>],
) -> Result<()> {
    work.terms = blockers.iter().try_fold(work.terms, |sum, blocker| {
        sum.checked_add(blocker.len())
            .ok_or_else(|| Error::InvalidInput("monotone proof term count overflows".into()))
    })?;
    if work.terms > limits.terms {
        return Err(Error::InvalidInput(
            "monotone proof terms exceed their limit".into(),
        ));
    }
    Ok(())
}

pub(super) fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| Error::InvalidInput("monotone selected cost overflows".into()))
    })
}

pub(super) fn check_branch_count(blockers: usize, children: usize) -> Result<()> {
    if blockers != children {
        return Err(Error::InvalidInput(
            "monotone branch child count differs from its blocker".into(),
        ));
    }
    Ok(())
}

pub(super) fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| Error::InvalidInput("monotone blocker bound overflows".into()))
    })
}

pub(super) fn proof_topology_checks(proof: &ProofNode) -> usize {
    match proof {
        ProofNode::Cost => 0,
        ProofNode::SurvivingMaximum | ProofNode::SurvivingSelectionLimit => 1,
        ProofNode::BlockerBound { blockers, .. } => blockers.len(),
        ProofNode::Branch { children, .. } => {
            1 + children.iter().map(proof_topology_checks).sum::<usize>()
        }
    }
}
