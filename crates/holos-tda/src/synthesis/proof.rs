use std::collections::BTreeSet;

use crate::monotone_search::SearchResult;
use crate::{Error, Result};

use super::model::{BoundKind, BuiltProof, ProofNode, SynthesisLimits, SynthesisStatus};
use super::oracle::{
    TopologyOracle, blocker_bound, blocker_min_cost, difference, insert_sorted, merge,
    selected_cost,
};
use super::{FORMAT_MAX_PROOF_NODES, FORMAT_MAX_PROOF_TERMS};

pub(super) fn build_synthesis_proof(
    status: SynthesisStatus,
    search: &SearchResult,
    costs: &[u64],
    max_edits: usize,
    action_count: usize,
    oracle: &TopologyOracle<'_>,
    limits: SynthesisLimits,
) -> Result<BuiltProof> {
    if status == SynthesisStatus::SearchIncomplete {
        return Ok(BuiltProof {
            proof: None,
            nodes: 0,
            topology_checks: 0,
        });
    }
    let cutoff = proof_cutoff(status, search.upper_bound)?;
    let mut builder = ProofBuilder::new(costs, max_edits, cutoff, oracle, limits);
    let proof = builder.prove(Vec::new(), (0..action_count).collect(), 0)?;
    Ok(BuiltProof {
        topology_checks: proof_topology_checks(&proof),
        proof: Some(proof),
        nodes: builder.nodes,
    })
}

pub(super) fn proof_cutoff(
    status: SynthesisStatus,
    upper_bound: Option<u64>,
) -> Result<Option<u64>> {
    match status {
        SynthesisStatus::Optimal => upper_bound
            .map(Some)
            .ok_or_else(|| Error::InvalidInput("optimal synthesis result has no cost".into())),
        SynthesisStatus::Infeasible | SynthesisStatus::SearchIncomplete => Ok(None),
    }
}

pub(super) struct ProofBuilder<'a> {
    costs: &'a [u64],
    max_edits: usize,
    cutoff: Option<u64>,
    oracle: &'a TopologyOracle<'a>,
    limits: SynthesisLimits,
    nodes: usize,
    topology_checks: usize,
    terms: usize,
}

impl<'a> ProofBuilder<'a> {
    pub(super) fn new(
        costs: &'a [u64],
        max_edits: usize,
        cutoff: Option<u64>,
        oracle: &'a TopologyOracle<'a>,
        limits: SynthesisLimits,
    ) -> Self {
        Self {
            costs,
            max_edits,
            cutoff,
            oracle,
            limits,
            nodes: 0,
            topology_checks: 0,
            terms: 0,
        }
    }

    pub(super) fn prove(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<ProofNode> {
        self.record_node(depth)?;
        let included_cost = selected_cost(self.costs, &included)?;
        if let Some(leaf) = self.early_leaf(&included, &available, included_cost)? {
            return Ok(leaf);
        }
        let blockers = self.pack_blockers(&included, &available)?;
        if blockers.is_empty() {
            return Err(Error::InvalidInput(
                "synthesis proof found an unreported feasible action set".into(),
            ));
        }
        if let Some(leaf) = self.blocker_leaf(&included, included_cost, &blockers)? {
            return Ok(leaf);
        }
        self.branch(included, available, blockers, depth)
    }

    pub(super) fn record_node(&mut self, depth: usize) -> Result<()> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("synthesis proof node count overflows".into()))?;
        if self.nodes > self.limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES)
            || depth > self.limits.max_proof_depth
        {
            return Err(Error::InvalidInput(
                "synthesis proof tree exceeds its node or depth limit".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn early_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
        included_cost: u64,
    ) -> Result<Option<ProofNode>> {
        if self.cutoff.is_some_and(|cutoff| included_cost >= cutoff) {
            return Ok(Some(ProofNode::Cost));
        }
        if included.len() == self.max_edits {
            if !self.check_survival(included)? {
                return Err(Error::InvalidInput(
                    "synthesis proof found a cheaper feasible action set".into(),
                ));
            }
            return Ok(Some(ProofNode::SurvivingEditLimit));
        }
        let maximum = merge(included, available);
        if self.check_survival(&maximum)? {
            return Ok(Some(ProofNode::SurvivingMaximum));
        }
        Ok(None)
    }

    pub(super) fn blocker_leaf(
        &mut self,
        included: &[usize],
        included_cost: u64,
        blockers: &[Vec<usize>],
    ) -> Result<Option<ProofNode>> {
        let cardinality = included.len().saturating_add(blockers.len());
        if cardinality > self.max_edits {
            self.add_blocker_terms(blockers)?;
            return Ok(Some(ProofNode::BlockerBound {
                kind: BoundKind::Edits,
                blockers: blockers.to_vec(),
            }));
        }
        let bound = included_cost
            .checked_add(blocker_bound(self.costs, blockers)?)
            .ok_or_else(|| Error::InvalidInput("synthesis proof cost bound overflows".into()))?;
        if self.cutoff.is_some_and(|cutoff| bound >= cutoff) {
            self.add_blocker_terms(blockers)?;
            return Ok(Some(ProofNode::BlockerBound {
                kind: BoundKind::Cost,
                blockers: blockers.to_vec(),
            }));
        }
        Ok(None)
    }

    pub(super) fn branch(
        &mut self,
        included: Vec<usize>,
        available: Vec<usize>,
        blockers: Vec<Vec<usize>>,
        depth: usize,
    ) -> Result<ProofNode> {
        let mut blocker = blockers
            .into_iter()
            .min_by_key(|blocker| (blocker.len(), blocker_min_cost(self.costs, blocker)))
            .expect("a nonempty packing has a blocker");
        blocker.sort_by_key(|candidate| (self.costs[*candidate], *candidate));
        self.add_blocker_terms(std::slice::from_ref(&blocker))?;
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

    pub(super) fn pack_blockers(
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

    pub(super) fn check_survival(&mut self, selected: &[usize]) -> Result<bool> {
        self.topology_checks = self.topology_checks.checked_add(1).ok_or_else(|| {
            Error::InvalidInput("synthesis proof topology count overflows".into())
        })?;
        if self.topology_checks > self.limits.max_oracle_calls {
            return Err(Error::InvalidInput(
                "synthesis proof topology checks exceed their limit".into(),
            ));
        }
        self.oracle.survives(selected)
    }

    pub(super) fn add_blocker_terms(&mut self, blockers: &[Vec<usize>]) -> Result<()> {
        self.terms = blockers.iter().try_fold(self.terms, |sum, blocker| {
            sum.checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("synthesis proof term count overflows".into()))
        })?;
        if self.terms > self.limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
            return Err(Error::InvalidInput(
                "synthesis proof terms exceed their limit".into(),
            ));
        }
        Ok(())
    }
}

pub(super) struct ProofVerifier<'a> {
    costs: &'a [u64],
    max_edits: usize,
    cutoff: Option<u64>,
    oracle: &'a TopologyOracle<'a>,
    limits: SynthesisLimits,
    pub(super) nodes: usize,
    pub(super) checks: usize,
    terms: usize,
}

impl<'a> ProofVerifier<'a> {
    pub(super) fn new(
        costs: &'a [u64],
        max_edits: usize,
        cutoff: Option<u64>,
        oracle: &'a TopologyOracle<'a>,
        limits: SynthesisLimits,
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

    pub(super) fn verify_root(&mut self, proof: &ProofNode) -> Result<()> {
        self.verify_node(proof, Vec::new(), (0..self.costs.len()).collect(), 0)
    }

    pub(super) fn verify_node(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<()> {
        self.record_node(depth)?;
        let included_cost = selected_cost(self.costs, &included)?;
        self.verify_node_kind(proof, included, available, included_cost, depth)
    }

    pub(super) fn record_node(&mut self, depth: usize) -> Result<()> {
        self.nodes = self
            .nodes
            .checked_add(1)
            .ok_or_else(|| Error::InvalidInput("synthesis proof node count overflows".into()))?;
        if self.nodes > self.limits.max_proof_nodes.min(FORMAT_MAX_PROOF_NODES)
            || depth > self.limits.max_proof_depth
        {
            return Err(Error::InvalidInput(
                "synthesis proof tree exceeds its node or depth limit".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn verify_node_kind(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        included_cost: u64,
        depth: usize,
    ) -> Result<()> {
        match proof {
            ProofNode::Cost => self.verify_cost_leaf(included_cost),
            ProofNode::SurvivingMaximum => self.verify_maximum_leaf(&included, &available),
            ProofNode::SurvivingEditLimit => self.verify_edit_leaf(&included),
            ProofNode::BlockerBound { kind, blockers } => {
                self.verify_bound_leaf(&included, &available, included_cost, *kind, blockers)
            }
            ProofNode::Branch { blocker, children } => {
                self.verify_branch(&included, &available, blocker, children, depth)
            }
        }
    }

    pub(super) fn verify_cost_leaf(&self, included_cost: u64) -> Result<()> {
        if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
            Err(Error::InvalidInput(
                "synthesis cost leaf does not reach the incumbent".into(),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn verify_maximum_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
    ) -> Result<()> {
        if !self.check_survival(&merge(included, available))? {
            Err(Error::InvalidInput(
                "synthesis maximal-survival leaf is feasible".into(),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn verify_edit_leaf(&mut self, included: &[usize]) -> Result<()> {
        if included.len() != self.max_edits || !self.check_survival(included)? {
            Err(Error::InvalidInput(
                "synthesis edit-limit leaf is invalid".into(),
            ))
        } else {
            Ok(())
        }
    }

    pub(super) fn verify_bound_leaf(
        &mut self,
        included: &[usize],
        available: &[usize],
        included_cost: u64,
        kind: BoundKind,
        blockers: &[Vec<usize>],
    ) -> Result<()> {
        self.verify_blockers(included, available, blockers)?;
        if self.bound_closes(included, included_cost, kind, blockers) {
            Ok(())
        } else {
            Err(Error::InvalidInput(
                "synthesis blocker leaf does not close its branch".into(),
            ))
        }
    }

    pub(super) fn bound_closes(
        &self,
        included: &[usize],
        included_cost: u64,
        kind: BoundKind,
        blockers: &[Vec<usize>],
    ) -> bool {
        match kind {
            BoundKind::Edits => included.len().saturating_add(blockers.len()) > self.max_edits,
            BoundKind::Cost => self.cutoff.is_some_and(|cutoff| {
                included_cost
                    .checked_add(blocker_bound(self.costs, blockers).unwrap_or(u64::MAX))
                    .is_some_and(|bound| bound >= cutoff)
            }),
        }
    }

    pub(super) fn verify_branch(
        &mut self,
        included: &[usize],
        available: &[usize],
        blocker: &[usize],
        children: &[ProofNode],
        depth: usize,
    ) -> Result<()> {
        self.verify_blockers(included, available, std::slice::from_ref(&blocker.to_vec()))?;
        if blocker.len() != children.len() {
            return Err(Error::InvalidInput(
                "synthesis branch child count differs from its blocker".into(),
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

    pub(super) fn verify_blockers(
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
                    "synthesis blocker family is not canonical and disjoint".into(),
                ));
            }
            let witness = merge(included, &difference(available, blocker));
            if !self.check_survival(&witness)? {
                return Err(Error::InvalidInput(
                    "synthesis blocker complement does not survive".into(),
                ));
            }
        }
        self.terms = blockers.iter().try_fold(self.terms, |sum, blocker| {
            sum.checked_add(blocker.len())
                .ok_or_else(|| Error::InvalidInput("synthesis proof term count overflows".into()))
        })?;
        if self.terms > self.limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
            return Err(Error::InvalidInput(
                "synthesis proof terms exceed their limit".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn check_survival(&mut self, selected: &[usize]) -> Result<bool> {
        self.checks = self.checks.checked_add(1).ok_or_else(|| {
            Error::InvalidInput("synthesis proof topology count overflows".into())
        })?;
        if self.checks > self.limits.max_oracle_calls {
            return Err(Error::InvalidInput(
                "synthesis proof topology checks exceed their limit".into(),
            ));
        }
        self.oracle.survives(selected)
    }
}

pub(super) fn proof_topology_checks(proof: &ProofNode) -> usize {
    match proof {
        ProofNode::Cost => 0,
        ProofNode::SurvivingMaximum | ProofNode::SurvivingEditLimit => 1,
        ProofNode::BlockerBound { blockers, .. } => blockers.len(),
        ProofNode::Branch { children, .. } => {
            1 + children.iter().map(proof_topology_checks).sum::<usize>()
        }
    }
}
