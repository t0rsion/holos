use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use crate::cohomology::{Edge, Space};
use crate::{MODULUS_LIMIT, ProofError, ProofLimits, Reader, is_prime};

const MAGIC: &[u8; 8] = b"HOLOSCI\0";
const VERSION: u16 = 2;
const F64_BITS_CODEC: u8 = 1;
const FORMAT_MAX_SCENARIOS: usize = 256;
const FORMAT_MAX_CANDIDATES: usize = 4_096;
const FORMAT_MAX_PROOF_TERMS: usize = 1_000_000;
const FORMAT_MAX_ORACLE_CALLS: usize = 10_000_000;
const FORMAT_MAX_SEARCH_NODES: usize = 10_000_000;

/// Whether bytes start with the cohomology-intervention magic.
pub fn is_cohomology_intervention(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Completeness status derived by independent weighted search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedCohomologyInterventionStatus {
    /// The selected edges have minimum total cost under the edit limit.
    Optimal,
    /// No selected edge set within the edit limit kills every target.
    Infeasible,
    /// An oracle-call or search-node limit stopped the proof.
    SearchIncomplete,
}

impl VerifiedCohomologyInterventionStatus {
    fn from_code(code: u8) -> Result<Self, ProofError> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(ProofError::new("cohomology intervention status is invalid")),
        }
    }
}

/// Counts and bounds from one independently checked intervention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCohomologyIntervention {
    /// Target cohomology dimension.
    pub dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Number of graph scenarios checked together.
    pub scenarios: usize,
    /// Verified search status.
    pub status: VerifiedCohomologyInterventionStatus,
    /// Number of selected edge additions.
    pub edits: usize,
    /// Total selected cost, when a feasible edit exists.
    pub total_cost: Option<u64>,
    /// Proved finite lower bound, when one exists.
    pub lower_bound_cost: Option<u64>,
    /// Feasible upper bound, when one exists.
    pub upper_bound_cost: Option<u64>,
    /// Distinct topological oracle calls.
    pub oracle_calls: usize,
    /// Branch-and-bound nodes visited.
    pub search_nodes: usize,
    /// Exact subset-cache hits.
    pub cache_hits: usize,
    /// Disjoint necessary candidate sets at the root.
    pub root_blockers: usize,
    /// Additive cost lower bound from those root blockers.
    pub root_blocker_bound: u64,
    /// Cohomology ranks before editing in scenario order.
    pub before_ranks: Vec<usize>,
    /// Cohomology ranks after the selected edit in scenario order.
    pub after_ranks: Vec<usize>,
}

#[derive(Debug, Clone)]
struct Scenario {
    edges: Vec<Edge>,
    target_basis: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Candidate {
    edge: Edge,
    cost: u64,
}

struct Claim {
    vertex_count: usize,
    dimension: usize,
    modulus: u32,
    scenarios: Vec<Scenario>,
    candidates: Vec<Candidate>,
    max_edits: usize,
    oracle_limit: usize,
    node_limit: usize,
    status: VerifiedCohomologyInterventionStatus,
    edits: Vec<usize>,
    lower_bound: Option<u64>,
    upper_bound: Option<u64>,
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
    root_blockers: Vec<Vec<usize>>,
    root_blocker_bound: u64,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
}

/// Verify a `HOLOSCI` artifact without linking to the producer crate.
///
/// The checker rebuilds every scenario cohomology space and restriction map.
/// It then repeats the weighted antitone search and its lower-bound packing.
pub fn verify_cohomology_intervention(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedCohomologyIntervention, ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "cohomology intervention exceeds its byte limit or is truncated",
        ));
    }
    let payload_len = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-cohomology-intervention-v2");
    hash.update(&bytes[..payload_len]);
    let expected: [u8; 32] = hash.finalize().into();
    if bytes[payload_len..] != expected {
        return Err(ProofError::new(
            "cohomology intervention digest differs from its content",
        ));
    }
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new(
            "unsupported cohomology intervention artifact",
        ));
    }
    let vertex_count = reader.bounded_usize("intervention vertex count", limits.max_vertices)?;
    let dimension = reader.bounded_usize("intervention dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    if !scale.is_finite() || scale < 0.0 {
        return Err(ProofError::new("cohomology intervention scale is invalid"));
    }
    if !is_prime(modulus as u64) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "cohomology intervention modulus is not a supported prime",
        ));
    }
    let scenario_count = reader.bounded_usize(
        "intervention scenario count",
        limits.max_snapshots.min(FORMAT_MAX_SCENARIOS),
    )?;
    if scenario_count == 0 {
        return Err(ProofError::new("cohomology intervention has no scenario"));
    }
    let mut scenarios = Vec::with_capacity(scenario_count);
    let mut total_edges = 0usize;
    for _ in 0..scenario_count {
        let edges = decode_edges(&mut reader, vertex_count, limits.max_edges)?;
        total_edges = total_edges
            .checked_add(edges.len())
            .ok_or_else(|| ProofError::new("intervention edge count overflows"))?;
        if total_edges > limits.max_edges {
            return Err(ProofError::new(
                "intervention scenario edges exceed their total limit",
            ));
        }
        scenarios.push(Scenario {
            edges,
            target_basis: reader.usize()?,
        });
    }
    let candidate_count = reader.bounded_usize(
        "intervention candidate count",
        limits.max_references.min(FORMAT_MAX_CANDIDATES),
    )?;
    if candidate_count > reader.remaining() / 24 {
        return Err(ProofError::new(
            "intervention candidates exceed the remaining bytes",
        ));
    }
    let mut candidates = Vec::with_capacity(candidate_count);
    for _ in 0..candidate_count {
        candidates.push(Candidate {
            edge: Edge {
                u: reader.usize()?,
                v: reader.usize()?,
            },
            cost: reader.u64()?,
        });
    }
    if candidates.iter().any(|candidate| {
        candidate.edge.u >= candidate.edge.v
            || candidate.edge.v >= vertex_count
            || candidate.cost == 0
    }) || candidates
        .windows(2)
        .any(|pair| pair[0].edge >= pair[1].edge)
    {
        return Err(ProofError::new(
            "cohomology intervention candidate list is not canonical",
        ));
    }
    for scenario in &scenarios {
        if candidates
            .iter()
            .any(|candidate| scenario.edges.binary_search(&candidate.edge).is_ok())
        {
            return Err(ProofError::new(
                "cohomology intervention candidate is active in a scenario",
            ));
        }
    }
    let max_edits = reader.usize()?;
    let oracle_limit = reader.usize()?;
    let node_limit = reader.usize()?;
    if max_edits > candidates.len()
        || oracle_limit == 0
        || oracle_limit > limits.max_snapshots.min(FORMAT_MAX_ORACLE_CALLS)
        || node_limit == 0
        || node_limit > limits.max_nodes.min(FORMAT_MAX_SEARCH_NODES)
    {
        return Err(ProofError::new(
            "cohomology intervention search limits are invalid",
        ));
    }
    let status = VerifiedCohomologyInterventionStatus::from_code(reader.u8()?)?;
    let edits = decode_indices(&mut reader, candidate_count, candidate_count)?;
    let lower_bound = optional_u64(&mut reader)?;
    let upper_bound = optional_u64(&mut reader)?;
    let oracle_calls = reader.usize()?;
    let search_nodes = reader.usize()?;
    let cache_hits = reader.usize()?;
    let blocker_count = reader.bounded_usize(
        "intervention blocker count",
        limits.max_terms.min(FORMAT_MAX_PROOF_TERMS),
    )?;
    let mut root_blockers = Vec::with_capacity(blocker_count);
    let mut proof_terms = 0usize;
    for _ in 0..blocker_count {
        let blocker = decode_indices(
            &mut reader,
            candidate_count,
            limits.max_terms.min(FORMAT_MAX_PROOF_TERMS),
        )?;
        proof_terms = proof_terms
            .checked_add(blocker.len())
            .ok_or_else(|| ProofError::new("intervention proof term count overflows"))?;
        if proof_terms > limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
            return Err(ProofError::new(
                "intervention proof terms exceed their limit",
            ));
        }
        root_blockers.push(blocker);
    }
    let root_blocker_bound = reader.u64()?;
    let before_ranks = decode_usizes(&mut reader, scenario_count)?;
    let after_ranks = decode_usizes(&mut reader, scenario_count)?;
    if reader.array32()? != expected || reader.remaining() != 0 {
        return Err(ProofError::new(
            "cohomology intervention has a wrong digest or trailing bytes",
        ));
    }
    let claim = Claim {
        vertex_count,
        dimension,
        modulus,
        scenarios,
        candidates,
        max_edits,
        oracle_limit,
        node_limit,
        status,
        edits,
        lower_bound,
        upper_bound,
        oracle_calls,
        search_nodes,
        cache_hits,
        root_blockers,
        root_blocker_bound,
        before_ranks,
        after_ranks,
    };
    let oracle = IndependentOracle::build(&claim, limits)?;
    let checked_before = oracle.spaces.iter().map(Space::rank).collect::<Vec<_>>();
    let costs = claim
        .candidates
        .iter()
        .map(|candidate| candidate.cost)
        .collect::<Vec<_>>();
    let search = Search::new(
        &costs,
        claim.max_edits,
        claim.oracle_limit,
        claim.node_limit,
        |selected| oracle.survives(selected),
    )
    .run()?;
    let checked_after = if search.selected.is_empty() {
        checked_before.clone()
    } else {
        oracle.ranks(&search.selected)?
    };
    if map_status(search.status) != claim.status
        || search.selected != claim.edits
        || search.lower_bound != claim.lower_bound
        || search.upper_bound != claim.upper_bound
        || search.oracle_calls != claim.oracle_calls
        || search.search_nodes != claim.search_nodes
        || search.cache_hits != claim.cache_hits
        || search.root_blockers != claim.root_blockers
        || search.root_blocker_bound != claim.root_blocker_bound
        || checked_before != claim.before_ranks
        || checked_after != claim.after_ranks
    {
        return Err(ProofError::new(
            "cohomology intervention differs from independent weighted search",
        ));
    }
    let total_cost = search.selected.iter().try_fold(0u64, |sum, position| {
        sum.checked_add(claim.candidates[*position].cost)
    });
    Ok(VerifiedCohomologyIntervention {
        dimension,
        modulus,
        scenarios: scenario_count,
        status,
        edits: claim.edits.len(),
        total_cost,
        lower_bound_cost: lower_bound,
        upper_bound_cost: upper_bound,
        oracle_calls,
        search_nodes,
        cache_hits,
        root_blockers: claim.root_blockers.len(),
        root_blocker_bound,
        before_ranks: claim.before_ranks.clone(),
        after_ranks: claim.after_ranks.clone(),
    })
}

struct IndependentOracle<'a> {
    claim: &'a Claim,
    spaces: Vec<Space>,
    limits: ProofLimits,
}

impl<'a> IndependentOracle<'a> {
    fn build(claim: &'a Claim, limits: ProofLimits) -> Result<Self, ProofError> {
        let spaces = claim
            .scenarios
            .iter()
            .map(|scenario| {
                Space::build(
                    claim.vertex_count,
                    claim.dimension,
                    &scenario.edges,
                    claim.modulus,
                    limits,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        for (scenario, space) in claim.scenarios.iter().zip(&spaces) {
            if scenario.target_basis >= space.rank() {
                return Err(ProofError::new(
                    "cohomology intervention target basis is out of range",
                ));
            }
        }
        Ok(Self {
            claim,
            spaces,
            limits,
        })
    }

    fn survives(&self, selected: &[usize]) -> Result<bool, ProofError> {
        for (position, scenario) in self.claim.scenarios.iter().enumerate() {
            let edited = self.edited_space(scenario, selected)?;
            if self.spaces[position].target_in_image_from(
                &edited,
                scenario.target_basis,
                self.claim.modulus,
            )? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ranks(&self, selected: &[usize]) -> Result<Vec<usize>, ProofError> {
        self.claim
            .scenarios
            .iter()
            .map(|scenario| {
                self.edited_space(scenario, selected)
                    .map(|space| space.rank())
            })
            .collect()
    }

    fn edited_space(&self, scenario: &Scenario, selected: &[usize]) -> Result<Space, ProofError> {
        let mut edges = scenario.edges.clone();
        edges.extend(
            selected
                .iter()
                .map(|position| self.claim.candidates[*position].edge),
        );
        edges.sort();
        Space::build(
            self.claim.vertex_count,
            self.claim.dimension,
            &edges,
            self.claim.modulus,
            self.limits,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SearchStatus {
    Optimal,
    Infeasible,
    Incomplete,
}

struct SearchResult {
    status: SearchStatus,
    selected: Vec<usize>,
    lower_bound: Option<u64>,
    upper_bound: Option<u64>,
    oracle_calls: usize,
    search_nodes: usize,
    cache_hits: usize,
    root_blockers: Vec<Vec<usize>>,
    root_blocker_bound: u64,
}

struct Search<'a, F> {
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

impl<'a, F> Search<'a, F>
where
    F: FnMut(&[usize]) -> Result<bool, ProofError>,
{
    fn new(
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

    fn run(&mut self) -> Result<SearchResult, ProofError> {
        let empty = Vec::new();
        let Some(empty_survives) = self.evaluate(&empty)? else {
            return self.result(SearchStatus::Incomplete, vec![], Some(0), None, vec![]);
        };
        if !empty_survives {
            return self.result(SearchStatus::Optimal, vec![], Some(0), Some(0), vec![]);
        }
        let all = (0..self.costs.len()).collect::<Vec<_>>();
        let Some(all_survives) = self.evaluate(&all)? else {
            return self.result(SearchStatus::Incomplete, vec![], Some(0), None, vec![]);
        };
        if all_survives {
            return self.result(SearchStatus::Infeasible, vec![], None, None, vec![]);
        }
        let root_packing = self.pack_blockers(&empty, &all)?;
        let root_bound = blocker_bound(self.costs, &root_packing.blockers)?;
        if root_packing.complete {
            let _ = self.greedy_upper(&all)?;
        }
        let complete = if root_packing.complete {
            self.explore(Vec::new(), all)?
        } else {
            false
        };
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
        self.result(status, selected, lower, upper, root_packing.blockers)
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
        let mut selected = Vec::new();
        while self.evaluate(&selected)?.is_some_and(|survives| survives) {
            if selected.len() == self.max_selected {
                return Ok(true);
            }
            let remaining = difference(available, &selected);
            let packing = self.pack_blockers(&selected, &remaining)?;
            let Some(blocker) = packing.blockers.first() else {
                return Ok(packing.complete);
            };
            let candidate = blocker
                .iter()
                .copied()
                .min_by_key(|candidate| (self.costs[*candidate], *candidate))
                .expect("a blocker is nonempty");
            insert_sorted(&mut selected, candidate);
            if !packing.complete {
                return Ok(false);
            }
        }
        if self.evaluate(&selected)?.is_none() {
            return Ok(false);
        }
        for candidate in selected.clone().into_iter().rev() {
            let reduced = without(&selected, candidate);
            let Some(survives) = self.evaluate(&reduced)? else {
                return Ok(false);
            };
            if !survives {
                selected = reduced;
            }
        }
        self.update_best(selected)?;
        Ok(true)
    }

    fn explore(&mut self, included: Vec<usize>, available: Vec<usize>) -> Result<bool, ProofError> {
        if self.search_nodes == self.node_limit {
            return Ok(false);
        }
        self.search_nodes += 1;
        let Some(survives) = self.evaluate(&included)? else {
            return Ok(false);
        };
        if !survives {
            self.update_best(included)?;
            return Ok(true);
        }
        if included.len() == self.max_selected {
            return Ok(true);
        }
        let included_cost = selected_cost(self.costs, &included)?;
        if self
            .best
            .as_ref()
            .is_some_and(|(best, _)| included_cost >= *best)
        {
            return Ok(true);
        }
        let union = merge(&included, &available);
        let Some(union_survives) = self.evaluate(&union)? else {
            return Ok(false);
        };
        if union_survives {
            return Ok(true);
        }
        let packing = self.pack_blockers(&included, &available)?;
        let bound = included_cost
            .checked_add(blocker_bound(self.costs, &packing.blockers)?)
            .ok_or_else(|| ProofError::new("intervention search bound overflows"))?;
        if included.len().saturating_add(packing.blockers.len()) > self.max_selected
            || self.best.as_ref().is_some_and(|(best, _)| bound >= *best)
        {
            return Ok(true);
        }
        if !packing.complete {
            return Ok(false);
        }
        let Some(mut branch) = packing
            .blockers
            .into_iter()
            .min_by_key(|blocker| (blocker.len(), blocker_min_cost(self.costs, blocker)))
        else {
            return Ok(true);
        };
        branch.sort_by_key(|candidate| (self.costs[*candidate], *candidate));
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

fn map_status(status: SearchStatus) -> VerifiedCohomologyInterventionStatus {
    match status {
        SearchStatus::Optimal => VerifiedCohomologyInterventionStatus::Optimal,
        SearchStatus::Infeasible => VerifiedCohomologyInterventionStatus::Infeasible,
        SearchStatus::Incomplete => VerifiedCohomologyInterventionStatus::SearchIncomplete,
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

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum: usize,
) -> Result<Vec<Edge>, ProofError> {
    let count = reader.bounded_usize("intervention edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(ProofError::new(
            "intervention edge count exceeds the remaining bytes",
        ));
    }
    let mut edges = Vec::with_capacity(count);
    for _ in 0..count {
        edges.push(Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        });
    }
    if edges
        .iter()
        .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new(
            "cohomology intervention edge list is not canonical",
        ));
    }
    Ok(edges)
}

fn decode_indices(
    reader: &mut Reader<'_>,
    candidates: usize,
    maximum: usize,
) -> Result<Vec<usize>, ProofError> {
    let values = decode_usizes(reader, maximum)?;
    if values.iter().any(|value| *value >= candidates)
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new(
            "intervention candidate indices are not canonical",
        ));
    }
    Ok(values)
}

fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("intervention integer count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(
            "intervention integer count exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

fn optional_u64(reader: &mut Reader<'_>) -> Result<Option<u64>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => Ok(Some(reader.u64()?)),
        _ => Err(ProofError::new(
            "intervention optional integer tag is invalid",
        )),
    }
}
