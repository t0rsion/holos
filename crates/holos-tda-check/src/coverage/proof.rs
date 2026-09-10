use std::collections::BTreeSet;

use crate::{ProofError, ProofLimits};

use super::evaluate::{
    add_terms, blocker_bound, difference, evaluate, insert_sorted, merge, selected_cost,
};
use super::model::{BoundKind, Claim, Evaluation, ProofNode};
use super::wire::{Reader, decode_indices};
use super::{FORMAT_MAX_PROOF_DEPTH, FORMAT_MAX_PROOF_NODES};

pub(crate) struct TreeVerifier<'a> {
    pub(crate) costs: &'a [u64],
    pub(crate) max_activations: usize,
    pub(crate) cutoff: Option<u64>,
    pub(crate) claim: &'a Claim,
    pub(crate) limits: ProofLimits,
    pub(crate) nodes: usize,
    pub(crate) checks: usize,
    pub(crate) terms: usize,
}

impl<'a> TreeVerifier<'a> {
    pub(crate) fn new(
        costs: &'a [u64],
        max_activations: usize,
        cutoff: Option<u64>,
        claim: &'a Claim,
        limits: ProofLimits,
    ) -> Self {
        Self {
            costs,
            max_activations,
            cutoff,
            claim,
            limits,
            nodes: 0,
            checks: 0,
            terms: 0,
        }
    }

    pub(crate) fn verify_root(&mut self, proof: &ProofNode) -> Result<(), ProofError> {
        self.verify_node(proof, Vec::new(), (0..self.costs.len()).collect(), 0)
    }

    pub(crate) fn verify_node(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<(), ProofError> {
        self.record_node(depth)?;
        let included_cost = selected_cost(self.costs, &included)?;
        match proof {
            ProofNode::Cost => self.verify_cost_leaf(included_cost),
            ProofNode::SurvivingMaximum => self.verify_maximum_leaf(&included, &available),
            ProofNode::SurvivingActivationLimit => self.verify_activation_leaf(&included),
            ProofNode::BlockerBound { kind, blockers } => {
                self.verify_bound_leaf(*kind, &included, &available, blockers, included_cost)
            }
            ProofNode::Branch { blocker, children } => {
                self.verify_branch(&included, &available, blocker, children, depth)
            }
        }
    }

    pub(crate) fn record_node(&mut self, depth: usize) -> Result<(), ProofError> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| ProofError::new("coverage proof node count overflows"))?;
        if self.nodes > self.limits.max_nodes.min(FORMAT_MAX_PROOF_NODES)
            || depth > FORMAT_MAX_PROOF_DEPTH
        {
            return Err(ProofError::new(
                "coverage proof tree exceeds its node or depth limit",
            ));
        }
        Ok(())
    }

    pub(crate) fn verify_cost_leaf(&self, included_cost: u64) -> Result<(), ProofError> {
        if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
            Err(ProofError::new(
                "coverage cost leaf does not reach the incumbent",
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn verify_maximum_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
    ) -> Result<(), ProofError> {
        if !self.check_survival(&merge(included, available))? {
            Err(ProofError::new(
                "coverage maximal-survival leaf is feasible",
            ))
        } else {
            Ok(())
        }
    }

    pub(crate) fn verify_activation_leaf(&mut self, included: &[usize]) -> Result<(), ProofError> {
        if included.len() != self.max_activations || !self.check_survival(included)? {
            Err(ProofError::new("coverage activation-limit leaf is invalid"))
        } else {
            Ok(())
        }
    }

    pub(crate) fn verify_bound_leaf(
        &mut self,
        kind: BoundKind,
        included: &[usize],
        available: &[usize],
        blockers: &[Vec<usize>],
        included_cost: u64,
    ) -> Result<(), ProofError> {
        self.verify_blockers(included, available, blockers)?;
        if self.bound_closes(kind, included.len(), blockers, included_cost) {
            Ok(())
        } else {
            Err(ProofError::new(
                "coverage blocker leaf does not close its branch",
            ))
        }
    }

    pub(crate) fn bound_closes(
        &self,
        kind: BoundKind,
        included: usize,
        blockers: &[Vec<usize>],
        included_cost: u64,
    ) -> bool {
        match kind {
            BoundKind::Activations => {
                included.saturating_add(blockers.len()) > self.max_activations
            }
            BoundKind::Cost => self.cutoff.is_some_and(|cutoff| {
                let add = blocker_bound(self.costs, blockers).unwrap_or(u64::MAX);
                included_cost
                    .checked_add(add)
                    .is_some_and(|bound| bound >= cutoff)
            }),
        }
    }

    pub(crate) fn verify_branch(
        &mut self,
        included: &[usize],
        available: &[usize],
        blocker: &[usize],
        children: &[ProofNode],
        depth: usize,
    ) -> Result<(), ProofError> {
        let blocker_family = [blocker.to_vec()];
        self.verify_blockers(included, available, &blocker_family)?;
        if blocker.len() != children.len() {
            return Err(ProofError::new(
                "coverage branch child count differs from its blocker",
            ));
        }
        let mut excluded = BTreeSet::new();
        for (&candidate, child) in blocker.iter().zip(children) {
            let mut child_included = included.to_vec();
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

    pub(crate) fn verify_blockers(
        &mut self,
        included: &[usize],
        available: &[usize],
        blockers: &[Vec<usize>],
    ) -> Result<(), ProofError> {
        let mut used = BTreeSet::new();
        for blocker in blockers {
            if blocker.is_empty()
                || blocker.iter().any(|candidate| {
                    available.binary_search(candidate).is_err() || !used.insert(*candidate)
                })
                || blocker.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(ProofError::new(
                    "coverage blocker family is not canonical and disjoint",
                ));
            }
            let witness = merge(included, &difference(available, blocker));
            if !self.check_survival(&witness)? {
                return Err(ProofError::new(
                    "coverage blocker complement does not survive",
                ));
            }
        }
        for blocker in blockers {
            add_terms(&mut self.terms, blocker.len(), self.limits)?;
        }
        Ok(())
    }

    pub(crate) fn check_survival(&mut self, selected: &[usize]) -> Result<bool, ProofError> {
        self.checks = self
            .checks
            .checked_add(1)
            .ok_or_else(|| ProofError::new("coverage proof topology count overflows"))?;
        if self.checks > self.limits.max_snapshots {
            return Err(ProofError::new(
                "coverage proof topology checks exceed their limit",
            ));
        }
        Ok(!evaluate(self.claim, selected, self.limits)?.criterion_holds)
    }
}

pub(crate) fn verify_root_blockers(
    claim: &Claim,
    blockers: &[Vec<usize>],
    costs: &[u64],
    lower_bound: Option<u64>,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    let available = (0..costs.len()).collect::<Vec<_>>();
    let mut used = BTreeSet::new();
    for blocker in blockers {
        if blocker.is_empty()
            || blocker
                .iter()
                .any(|candidate| *candidate >= costs.len() || !used.insert(*candidate))
            || blocker.windows(2).any(|pair| pair[0] >= pair[1])
            || evaluate(claim, &difference(&available, blocker), limits)?.criterion_holds
        {
            return Err(ProofError::new(
                "coverage root blocker does not certify a necessary activation set",
            ));
        }
    }
    if lower_bound.is_some_and(|lower| match blocker_bound(costs, blockers) {
        Ok(bound) => lower < bound,
        Err(_) => true,
    }) {
        return Err(ProofError::new(
            "coverage lower bound is below its root blocker certificate",
        ));
    }
    Ok(())
}

pub(crate) fn decode_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<ProofNode, ProofError> {
    record_decoded_node(nodes, depth, limits)?;
    match reader.u8()? {
        1 => Ok(ProofNode::Cost),
        2 => Ok(ProofNode::SurvivingMaximum),
        3 => Ok(ProofNode::SurvivingActivationLimit),
        4 => decode_bound_node(reader, action_count, terms, limits),
        5 => decode_branch_node(reader, action_count, depth, nodes, terms, limits),
        _ => Err(ProofError::new("coverage proof node kind is invalid")),
    }
}

pub(crate) fn record_decoded_node(
    nodes: &mut usize,
    depth: usize,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| ProofError::new("coverage proof node count overflows"))?;
    if *nodes > limits.max_nodes.min(FORMAT_MAX_PROOF_NODES) || depth > FORMAT_MAX_PROOF_DEPTH {
        return Err(ProofError::new(
            "coverage proof tree exceeds its node or depth limit",
        ));
    }
    Ok(())
}

pub(crate) fn decode_bound_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<ProofNode, ProofError> {
    let kind = decode_bound_kind(reader)?;
    let blockers = decode_proof_blockers(reader, action_count, terms, limits)?;
    Ok(ProofNode::BlockerBound { kind, blockers })
}

pub(crate) fn decode_bound_kind(reader: &mut Reader<'_>) -> Result<BoundKind, ProofError> {
    match reader.u8()? {
        1 => Ok(BoundKind::Cost),
        2 => Ok(BoundKind::Activations),
        _ => Err(ProofError::new("coverage proof bound kind is invalid")),
    }
}

pub(crate) fn decode_proof_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<usize>>, ProofError> {
    let count = reader.bounded_usize("proof blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_terms(terms, blocker.len(), limits)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

pub(crate) fn decode_branch_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<ProofNode, ProofError> {
    let blocker = decode_indices(reader, action_count, limits.max_terms)?;
    add_terms(terms, blocker.len(), limits)?;
    let child_count = reader.bounded_usize("proof child count", action_count)?;
    if child_count != blocker.len() {
        return Err(ProofError::new(
            "coverage branch child count differs from its blocker",
        ));
    }
    let children = decode_children(
        reader,
        action_count,
        child_count,
        depth + 1,
        nodes,
        terms,
        limits,
    )?;
    Ok(ProofNode::Branch { blocker, children })
}

pub(crate) fn decode_children(
    reader: &mut Reader<'_>,
    action_count: usize,
    count: usize,
    depth: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<ProofNode>, ProofError> {
    let mut children = Vec::with_capacity(count);
    for _ in 0..count {
        children.push(decode_proof(
            reader,
            action_count,
            depth,
            nodes,
            terms,
            limits,
        )?);
    }
    Ok(children)
}

pub(crate) fn decode_evaluation(reader: &mut Reader<'_>) -> Result<Evaluation, ProofError> {
    let criterion_holds = match reader.u8()? {
        0 => false,
        1 => true,
        _ => return Err(ProofError::new("coverage evaluation Boolean is invalid")),
    };
    Ok(Evaluation {
        criterion_holds,
        checks: reader.usize()?,
        minimum_witness: reader.optional_usize()?,
    })
}
