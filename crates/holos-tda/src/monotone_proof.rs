//! Proof trees for exact optimization over an antitone survival predicate.

use std::collections::BTreeSet;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundKind {
    Cost,
    Selections,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProofNode {
    Cost,
    SurvivingMaximum,
    SurvivingSelectionLimit,
    BlockerBound {
        kind: BoundKind,
        blockers: Vec<Vec<usize>>,
    },
    Branch {
        blocker: Vec<usize>,
        children: Vec<ProofNode>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProofLimits {
    pub nodes: usize,
    pub depth: usize,
    pub terms: usize,
    pub checks: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ProofWork {
    pub nodes: usize,
    pub checks: usize,
    pub terms: usize,
}

pub(crate) fn build_proof<F>(
    costs: &[u64],
    max_selected: usize,
    cutoff: Option<u64>,
    limits: ProofLimits,
    oracle: &mut F,
) -> Result<(ProofNode, ProofWork)>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    let mut builder = Builder {
        costs,
        max_selected,
        cutoff,
        limits,
        oracle,
        work: ProofWork::default(),
    };
    let proof = builder.prove(Vec::new(), (0..costs.len()).collect(), 0)?;
    let work = ProofWork {
        checks: proof_topology_checks(&proof),
        ..builder.work
    };
    Ok((proof, work))
}

pub(crate) fn verify_proof<F>(
    proof: &ProofNode,
    costs: &[u64],
    max_selected: usize,
    cutoff: Option<u64>,
    limits: ProofLimits,
    oracle: &mut F,
) -> Result<ProofWork>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    let mut verifier = Verifier {
        costs,
        max_selected,
        cutoff,
        limits,
        oracle,
        work: ProofWork::default(),
    };
    verifier.verify_node(proof, Vec::new(), (0..costs.len()).collect(), 0)?;
    Ok(verifier.work)
}

pub(crate) fn verify_root_blockers<F>(
    blockers: &[Vec<usize>],
    costs: &[u64],
    lower_bound: Option<u64>,
    oracle: &mut F,
) -> Result<()>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    let available = (0..costs.len()).collect::<Vec<_>>();
    let mut used = BTreeSet::new();
    for blocker in blockers {
        if blocker.is_empty()
            || blocker
                .iter()
                .any(|candidate| *candidate >= costs.len() || !used.insert(*candidate))
            || blocker.windows(2).any(|pair| pair[0] >= pair[1])
            || !oracle(&difference(&available, blocker))?
        {
            return Err(Error::InvalidInput(
                "monotone root blocker is not a necessary selection set".into(),
            ));
        }
    }
    let bound = blocker_bound(costs, blockers)?;
    if lower_bound.is_some_and(|lower| lower < bound) {
        return Err(Error::InvalidInput(
            "monotone lower bound is below its blocker certificate".into(),
        ));
    }
    Ok(())
}

pub(crate) fn proof_topology_checks(proof: &ProofNode) -> usize {
    match proof {
        ProofNode::Cost => 0,
        ProofNode::SurvivingMaximum | ProofNode::SurvivingSelectionLimit => 1,
        ProofNode::BlockerBound { blockers, .. } => blockers.len(),
        ProofNode::Branch { children, .. } => {
            1 + children.iter().map(proof_topology_checks).sum::<usize>()
        }
    }
}

struct Builder<'a, F> {
    costs: &'a [u64],
    max_selected: usize,
    cutoff: Option<u64>,
    limits: ProofLimits,
    oracle: &'a mut F,
    work: ProofWork,
}

impl<F> Builder<'_, F>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    fn prove(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<ProofNode> {
        self.work.nodes = self
            .work
            .nodes
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("monotone proof node count overflows".into()))?;
        if self.work.nodes > self.limits.nodes || depth > self.limits.depth {
            return Err(Error::InvalidInput(
                "monotone proof exceeds its node or depth limit".into(),
            ));
        }
        let included_cost = selected_cost(self.costs, &included)?;
        if self.cutoff.is_some_and(|cutoff| included_cost >= cutoff) {
            return Ok(ProofNode::Cost);
        }
        if included.len() == self.max_selected {
            if !self.check_survival(&included)? {
                return Err(Error::InvalidInput(
                    "monotone proof found a cheaper feasible selection".into(),
                ));
            }
            return Ok(ProofNode::SurvivingSelectionLimit);
        }
        let maximum = merge(&included, &available);
        if self.check_survival(&maximum)? {
            return Ok(ProofNode::SurvivingMaximum);
        }
        let blockers = self.pack_blockers(&included, &available)?;
        if blockers.is_empty() {
            return Err(Error::InvalidInput(
                "monotone proof found an unreported feasible selection".into(),
            ));
        }
        if included.len().saturating_add(blockers.len()) > self.max_selected {
            self.add_terms(&blockers)?;
            return Ok(ProofNode::BlockerBound {
                kind: BoundKind::Selections,
                blockers,
            });
        }
        let bound = included_cost
            .checked_add(blocker_bound(self.costs, &blockers)?)
            .ok_or_else(|| Error::InvalidInput("monotone proof cost bound overflows".into()))?;
        if self.cutoff.is_some_and(|cutoff| bound >= cutoff) {
            self.add_terms(&blockers)?;
            return Ok(ProofNode::BlockerBound {
                kind: BoundKind::Cost,
                blockers,
            });
        }
        let blocker = blockers
            .into_iter()
            .min_by_key(|blocker| (blocker.len(), blocker_min_cost(self.costs, blocker)))
            .expect("a nonempty packing has a blocker");
        self.add_terms(std::slice::from_ref(&blocker))?;
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
        while self.check_survival(&merge(included, &used))? {
            let mut retained = used.clone();
            for candidate in available
                .iter()
                .copied()
                .filter(|candidate| used.binary_search(candidate).is_err())
            {
                let mut trial = merge(included, &retained);
                insert_sorted(&mut trial, candidate);
                if self.check_survival(&trial)? {
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

    fn check_survival(&mut self, selected: &[usize]) -> Result<bool> {
        self.work.checks =
            self.work.checks.checked_add(1).ok_or_else(|| {
                Error::InvalidInput("monotone proof check count overflows".into())
            })?;
        if self.work.checks > self.limits.checks {
            return Err(Error::InvalidInput(
                "monotone proof checks exceed their limit".into(),
            ));
        }
        (self.oracle)(selected)
    }

    fn add_terms(&mut self, blockers: &[Vec<usize>]) -> Result<()> {
        self.work.terms = blockers.iter().try_fold(self.work.terms, |sum, blocker| {
            sum.checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("monotone proof term count overflows".into()))
        })?;
        if self.work.terms > self.limits.terms {
            return Err(Error::InvalidInput(
                "monotone proof terms exceed their limit".into(),
            ));
        }
        Ok(())
    }
}

struct Verifier<'a, F> {
    costs: &'a [u64],
    max_selected: usize,
    cutoff: Option<u64>,
    limits: ProofLimits,
    oracle: &'a mut F,
    work: ProofWork,
}

impl<F> Verifier<'_, F>
where
    F: FnMut(&[usize]) -> Result<bool>,
{
    fn verify_node(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<()> {
        self.work.nodes = self
            .work
            .nodes
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("monotone proof node count overflows".into()))?;
        if self.work.nodes > self.limits.nodes || depth > self.limits.depth {
            return Err(Error::InvalidInput(
                "monotone proof exceeds its node or depth limit".into(),
            ));
        }
        let included_cost = selected_cost(self.costs, &included)?;
        match proof {
            ProofNode::Cost => {
                if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
                    return Err(Error::InvalidInput(
                        "monotone cost leaf does not reach the incumbent".into(),
                    ));
                }
            }
            ProofNode::SurvivingMaximum => {
                if !self.check_survival(&merge(&included, &available))? {
                    return Err(Error::InvalidInput(
                        "monotone maximal-survival leaf is feasible".into(),
                    ));
                }
            }
            ProofNode::SurvivingSelectionLimit => {
                if included.len() != self.max_selected || !self.check_survival(&included)? {
                    return Err(Error::InvalidInput(
                        "monotone selection-limit leaf is invalid".into(),
                    ));
                }
            }
            ProofNode::BlockerBound { kind, blockers } => {
                self.verify_blockers(&included, &available, blockers)?;
                match kind {
                    BoundKind::Selections
                        if included.len().saturating_add(blockers.len()) > self.max_selected => {}
                    BoundKind::Cost
                        if self.cutoff.is_some_and(|cutoff| {
                            included_cost
                                .checked_add(
                                    blocker_bound(self.costs, blockers).unwrap_or(u64::MAX),
                                )
                                .is_some_and(|bound| bound >= cutoff)
                        }) => {}
                    _ => {
                        return Err(Error::InvalidInput(
                            "monotone blocker leaf does not close its branch".into(),
                        ));
                    }
                }
            }
            ProofNode::Branch { blocker, children } => {
                self.verify_blockers(&included, &available, std::slice::from_ref(blocker))?;
                if blocker.len() != children.len() {
                    return Err(Error::InvalidInput(
                        "monotone branch child count differs from its blocker".into(),
                    ));
                }
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
            }
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
            if !self.check_survival(&witness)? {
                return Err(Error::InvalidInput(
                    "monotone blocker complement does not survive".into(),
                ));
            }
        }
        self.work.terms = blockers.iter().try_fold(self.work.terms, |sum, blocker| {
            sum.checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("monotone proof term count overflows".into()))
        })?;
        if self.work.terms > self.limits.terms {
            return Err(Error::InvalidInput(
                "monotone proof terms exceed their limit".into(),
            ));
        }
        Ok(())
    }

    fn check_survival(&mut self, selected: &[usize]) -> Result<bool> {
        self.work.checks =
            self.work.checks.checked_add(1).ok_or_else(|| {
                Error::InvalidInput("monotone proof check count overflows".into())
            })?;
        if self.work.checks > self.limits.checks {
            return Err(Error::InvalidInput(
                "monotone proof checks exceed their limit".into(),
            ));
        }
        (self.oracle)(selected)
    }
}

fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| Error::InvalidInput("monotone selected cost overflows".into()))
    })
}

fn blocker_min_cost(costs: &[u64], blocker: &[usize]) -> u64 {
    blocker
        .iter()
        .map(|candidate| costs[*candidate])
        .min()
        .unwrap_or(0)
}

fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| Error::InvalidInput("monotone blocker bound overflows".into()))
    })
}

fn insert_sorted(values: &mut Vec<usize>, value: usize) {
    if let Err(position) = values.binary_search(&value) {
        values.insert(position, value);
    }
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

    #[test]
    fn recursive_proof_excludes_every_cheaper_hitting_set() {
        let costs = [1, 1, 1];
        let groups = [vec![0, 1], vec![1, 2], vec![0, 2]];
        let mut oracle = |selected: &[usize]| {
            Ok(groups.iter().any(|group| {
                group
                    .iter()
                    .all(|candidate| selected.binary_search(candidate).is_err())
            }))
        };
        let limits = ProofLimits {
            nodes: 100,
            depth: 10,
            terms: 100,
            checks: 100,
        };
        let (proof, built) = build_proof(&costs, 2, Some(2), limits, &mut oracle).unwrap();
        assert!(built.nodes > 1);
        let checked = verify_proof(&proof, &costs, 2, Some(2), limits, &mut oracle).unwrap();
        assert_eq!(checked.checks, proof_topology_checks(&proof));
    }
}
