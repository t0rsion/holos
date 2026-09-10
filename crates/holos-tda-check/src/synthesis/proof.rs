use std::collections::BTreeSet;

use crate::{ProofError, ProofLimits};

use super::model::{BoundKind, FORMAT_MAX_PROOF_DEPTH, FORMAT_MAX_PROOF_NODES, ProofNode};
use super::oracle::Oracle;
use super::wire::{Reader, add_terms, decode_indices};

pub(super) fn decode_optional_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    nodes: &mut usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<Option<ProofNode>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_proof(reader, action_count, 0, nodes, terms, limits).map(Some),
        _ => Err(ProofError::new("synthesis proof-presence flag is invalid")),
    }
}

pub(super) struct TreeVerifier<'a> {
    costs: &'a [u64],
    max_edits: usize,
    cutoff: Option<u64>,
    oracle: &'a Oracle<'a>,
    limits: ProofLimits,
    pub(super) nodes: usize,
    pub(super) checks: usize,
    terms: usize,
}

impl<'a> TreeVerifier<'a> {
    pub(super) fn new(
        costs: &'a [u64],
        max_edits: usize,
        cutoff: Option<u64>,
        oracle: &'a Oracle<'a>,
        limits: ProofLimits,
    ) -> Self {
        Self {
            costs,
            max_edits,
            cutoff,
            oracle,
            limits,
            nodes: 0,
            checks: 0,
            terms: 0,
        }
    }

    pub(super) fn verify_root(&mut self, proof: &ProofNode) -> Result<(), ProofError> {
        self.verify_node(proof, Vec::new(), (0..self.costs.len()).collect(), 0)
    }

    fn verify_node(
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
            ProofNode::SurvivingEditLimit => self.verify_edit_leaf(&included),
            ProofNode::BlockerBound { kind, blockers } => {
                self.verify_bound_leaf(*kind, &included, &available, blockers, included_cost)
            }
            ProofNode::Branch { blocker, children } => {
                self.verify_branch(&included, &available, blocker, children, depth)
            }
        }
    }

    fn record_node(&mut self, depth: usize) -> Result<(), ProofError> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| ProofError::new("synthesis proof node count overflows"))?;
        if self.nodes > self.limits.max_nodes.min(FORMAT_MAX_PROOF_NODES)
            || depth > FORMAT_MAX_PROOF_DEPTH
        {
            return Err(ProofError::new(
                "synthesis proof tree exceeds its node or depth limit",
            ));
        }
        Ok(())
    }

    fn verify_cost_leaf(&self, included_cost: u64) -> Result<(), ProofError> {
        if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
            Err(ProofError::new(
                "synthesis cost leaf does not reach the incumbent",
            ))
        } else {
            Ok(())
        }
    }

    fn verify_maximum_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
    ) -> Result<(), ProofError> {
        if !self.check_survival(&merge(included, available))? {
            Err(ProofError::new(
                "synthesis maximal-survival leaf is feasible",
            ))
        } else {
            Ok(())
        }
    }

    fn verify_edit_leaf(&mut self, included: &[usize]) -> Result<(), ProofError> {
        if included.len() != self.max_edits || !self.check_survival(included)? {
            Err(ProofError::new("synthesis edit-limit leaf is invalid"))
        } else {
            Ok(())
        }
    }

    fn verify_bound_leaf(
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
                "synthesis blocker leaf does not close its branch",
            ))
        }
    }

    fn bound_closes(
        &self,
        kind: BoundKind,
        included: usize,
        blockers: &[Vec<usize>],
        included_cost: u64,
    ) -> bool {
        match kind {
            BoundKind::Edits => included.saturating_add(blockers.len()) > self.max_edits,
            BoundKind::Cost => self.cutoff.is_some_and(|cutoff| {
                let add = blocker_bound(self.costs, blockers).unwrap_or(u64::MAX);
                included_cost
                    .checked_add(add)
                    .is_some_and(|bound| bound >= cutoff)
            }),
        }
    }

    fn verify_branch(
        &mut self,
        included: &[usize],
        available: &[usize],
        blocker: &[usize],
        children: &[ProofNode],
        depth: usize,
    ) -> Result<(), ProofError> {
        self.verify_blockers(included, available, std::slice::from_ref(&blocker.to_vec()))?;
        if blocker.len() != children.len() {
            return Err(ProofError::new(
                "synthesis branch child count differs from its blocker",
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

    fn verify_blockers(
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
                    "synthesis blocker family is not canonical and disjoint",
                ));
            }
            let witness = merge(included, &difference(available, blocker));
            if !self.check_survival(&witness)? {
                return Err(ProofError::new(
                    "synthesis blocker complement does not survive",
                ));
            }
        }
        for blocker in blockers {
            add_terms(&mut self.terms, blocker.len(), self.limits)?;
        }
        Ok(())
    }

    fn check_survival(&mut self, selected: &[usize]) -> Result<bool, ProofError> {
        self.checks = self
            .checks
            .checked_add(1)
            .ok_or_else(|| ProofError::new("synthesis proof topology count overflows"))?;
        if self.checks > self.limits.max_snapshots {
            return Err(ProofError::new(
                "synthesis proof topology checks exceed their limit",
            ));
        }
        self.oracle.survives(selected)
    }
}

pub(super) fn verify_root_blockers(
    oracle: &Oracle<'_>,
    blockers: &[Vec<usize>],
    costs: &[u64],
    lower_bound: Option<u64>,
) -> Result<(), ProofError> {
    let available = (0..costs.len()).collect::<Vec<_>>();
    let mut used = BTreeSet::new();
    for blocker in blockers {
        if blocker.is_empty()
            || blocker
                .iter()
                .any(|candidate| *candidate >= costs.len() || !used.insert(*candidate))
            || blocker.windows(2).any(|pair| pair[0] >= pair[1])
            || !oracle.survives(&difference(&available, blocker))?
        {
            return Err(ProofError::new(
                "synthesis root blocker does not certify a necessary action set",
            ));
        }
    }
    if lower_bound.is_some_and(|lower| match blocker_bound(costs, blockers) {
        Ok(bound) => lower < bound,
        Err(_) => true,
    }) {
        return Err(ProofError::new(
            "synthesis lower bound is below its root blocker certificate",
        ));
    }
    Ok(())
}

fn decode_proof(
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
        3 => Ok(ProofNode::SurvivingEditLimit),
        4 => decode_bound_node(reader, action_count, terms, limits),
        5 => decode_branch_node(reader, action_count, depth, nodes, terms, limits),
        _ => Err(ProofError::new("synthesis proof node kind is invalid")),
    }
}

fn record_decoded_node(
    nodes: &mut usize,
    depth: usize,
    limits: ProofLimits,
) -> Result<(), ProofError> {
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| ProofError::new("synthesis proof node count overflows"))?;
    if *nodes > limits.max_nodes.min(FORMAT_MAX_PROOF_NODES) || depth > FORMAT_MAX_PROOF_DEPTH {
        return Err(ProofError::new(
            "synthesis proof tree exceeds its node or depth limit",
        ));
    }
    Ok(())
}

fn decode_bound_node(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<ProofNode, ProofError> {
    let kind = decode_bound_kind(reader)?;
    let blockers = decode_proof_blockers(reader, action_count, terms, limits)?;
    Ok(ProofNode::BlockerBound { kind, blockers })
}

fn decode_bound_kind(reader: &mut Reader<'_>) -> Result<BoundKind, ProofError> {
    match reader.u8()? {
        1 => Ok(BoundKind::Cost),
        2 => Ok(BoundKind::Edits),
        _ => Err(ProofError::new("synthesis proof bound kind is invalid")),
    }
}

fn decode_proof_blockers(
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

fn decode_branch_node(
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
            "synthesis branch child count differs from its blocker",
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

fn decode_children(
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

pub(super) fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64, ProofError> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| ProofError::new("synthesis selected cost overflows"))
    })
}

fn blocker_min_cost(costs: &[u64], blocker: &[usize]) -> u64 {
    blocker
        .iter()
        .map(|candidate| costs[*candidate])
        .min()
        .unwrap_or(0)
}

pub(super) fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64, ProofError> {
    blockers.iter().try_fold(0u64, |sum, blocker| {
        sum.checked_add(blocker_min_cost(costs, blocker))
            .ok_or_else(|| ProofError::new("synthesis blocker bound overflows"))
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
