//! Proof trees for exact optimization over an antitone survival predicate.

mod builder;
mod helpers;
mod verifier;

#[cfg(test)]
mod tests;

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
    let mut builder = builder::Builder {
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
    let mut verifier = verifier::Verifier {
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
            || !oracle(&helpers::difference(&available, blocker))?
        {
            return Err(Error::InvalidInput(
                "monotone root blocker is not a necessary selection set".into(),
            ));
        }
    }
    let bound = helpers::blocker_bound(costs, blockers)?;
    if lower_bound.is_some_and(|lower| lower < bound) {
        return Err(Error::InvalidInput(
            "monotone lower bound is below its blocker certificate".into(),
        ));
    }
    Ok(())
}

pub(crate) fn proof_topology_checks(proof: &ProofNode) -> usize {
    helpers::proof_topology_checks(proof)
}
