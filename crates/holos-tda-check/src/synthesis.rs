use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use num_rational::BigRational;
use sha2::{Digest, Sha256};

use crate::cohomology::{Edge, MapTerm, Space};
use crate::{ProofError, ProofLimits};

const MAGIC: &[u8; 8] = b"HOLOSSYN";
const VERSION: u16 = 1;
const F64_BITS_CODEC: u8 = 1;
const MODULUS_LIMIT: u64 = 32_768;
const FORMAT_MAX_STATES: usize = 4_096;
const FORMAT_MAX_ACTIONS: usize = 65_536;
const FORMAT_MAX_ORACLE_CALLS: usize = 10_000_000;
const FORMAT_MAX_SEARCH_NODES: usize = 10_000_000;
const FORMAT_MAX_PROOF_NODES: usize = 10_000_000;
const FORMAT_MAX_PROOF_TERMS: usize = 100_000_000;
const FORMAT_MAX_PROOF_DEPTH: usize = 4_096;

/// Completeness status accepted from a synthesis proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedSynthesisStatus {
    /// The checked action set has minimum total cost.
    Optimal,
    /// No checked action set satisfies every state under the edit limit.
    Infeasible,
    /// The producer stopped with a checked bound or incumbent.
    SearchIncomplete,
}

/// Completeness scope accepted from a synthesis proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedSynthesisSource {
    /// The claim covers only its listed states.
    Finite,
    /// The checker reconstructed all fixed-scale states of an affine trajectory.
    Affine,
}

impl VerifiedSynthesisStatus {
    fn from_code(code: u8) -> Result<Self, ProofError> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(ProofError::new("synthesis status is invalid")),
        }
    }
}

/// Summary of an independently checked synthesis result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedSynthesis {
    /// Cohomology dimension.
    pub dimension: usize,
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Completeness scope of the checked state list.
    pub source: VerifiedSynthesisSource,
    /// Number of checked temporal states.
    pub states: usize,
    /// Number of declared actions.
    pub actions: usize,
    /// Completeness status.
    pub status: VerifiedSynthesisStatus,
    /// Number of selected actions.
    pub selected: usize,
    /// Selected action cost, when an incumbent exists.
    pub total_cost: Option<u64>,
    /// Checked lower cost bound, when finite.
    pub lower_bound_cost: Option<u64>,
    /// Checked incumbent cost, when present.
    pub upper_bound_cost: Option<u64>,
    /// Producer topology calls recorded in the artifact.
    pub producer_oracle_calls: usize,
    /// Producer branch nodes recorded in the artifact.
    pub producer_search_nodes: usize,
    /// Proof-tree node count.
    pub proof_nodes: usize,
    /// Topology checks made while validating the proof tree.
    pub proof_topology_checks: usize,
    /// Target subspace ranks before editing.
    pub before_ranks: Vec<usize>,
    /// Surviving target ranks after editing.
    pub after_ranks: Vec<usize>,
}

/// Return true when bytes start with a synthesis envelope.
pub fn is_synthesis(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify one bounded synthesis proof without invoking `holos-tda`.
pub fn verify_synthesis(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<VerifiedSynthesis, ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "synthesis artifact exceeds its byte limit or is truncated",
        ));
    }
    let payload_end = bytes.len() - 32;
    let mut hash = Sha256::new();
    hash.update(b"holos-synthesis-artifact-v1");
    hash.update(&bytes[..payload_end]);
    let expected: [u8; 32] = hash.finalize().into();
    let mut reader = Reader::new(bytes);
    if reader.take(8)? != MAGIC || reader.u16()? != VERSION || reader.u8()? != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported synthesis artifact"));
    }
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let dimension = reader.bounded_usize("dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    if !scale.is_finite()
        || scale < 0.0
        || !is_prime(modulus as u64)
        || u64::from(modulus) >= MODULUS_LIMIT
    {
        return Err(ProofError::new(
            "synthesis scale or coefficient field is invalid",
        ));
    }
    let source = decode_source(&mut reader, limits)?;
    let state_count =
        reader.bounded_usize("state count", limits.max_snapshots.min(FORMAT_MAX_STATES))?;
    let mut states = Vec::with_capacity(state_count);
    let mut edge_count = 0usize;
    let mut target_terms = 0usize;
    let mut prior = None;
    for _ in 0..state_count {
        let scenario = reader.u64()?;
        let step = reader.u64()?;
        if prior.is_some_and(|key| key >= (scenario, step)) {
            return Err(ProofError::new(
                "synthesis states are not in canonical scenario and step order",
            ));
        }
        prior = Some((scenario, step));
        let edges = decode_edges(&mut reader, vertex_count, limits.max_edges)?;
        edge_count = edge_count
            .checked_add(edges.len())
            .ok_or_else(|| ProofError::new("synthesis edge count overflows"))?;
        if edge_count > limits.max_edges {
            return Err(ProofError::new(
                "synthesis state edges exceed their total limit",
            ));
        }
        let target_space = reader.array32()?;
        let row_count = reader.bounded_usize("target row count", limits.max_terms)?;
        let mut target = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            let term_count = reader.bounded_usize("target row term count", limits.max_terms)?;
            target_terms = target_terms
                .checked_add(term_count)
                .ok_or_else(|| ProofError::new("synthesis target term count overflows"))?;
            if target_terms > limits.max_terms || term_count > reader.remaining() / 12 {
                return Err(ProofError::new(
                    "synthesis target terms exceed their limit or remaining bytes",
                ));
            }
            let mut row = Vec::with_capacity(term_count);
            for _ in 0..term_count {
                row.push(MapTerm {
                    target: reader.usize()?,
                    coefficient: reader.u32()?,
                });
            }
            target.push(row);
        }
        states.push(State {
            scenario,
            step,
            edges,
            target_space,
            target,
            max_surviving_rank: reader.usize()?,
        });
    }
    let action_count = reader.bounded_usize(
        "action count",
        limits.max_references.min(FORMAT_MAX_ACTIONS),
    )?;
    let mut actions = Vec::with_capacity(action_count);
    for _ in 0..action_count {
        actions.push(Action {
            edge: Edge {
                u: reader.usize()?,
                v: reader.usize()?,
            },
            cost: reader.u64()?,
            states: decode_indices(&mut reader, state_count, state_count)?,
        });
    }
    if actions.windows(2).any(|pair| pair[0] >= pair[1])
        || actions.iter().any(|action| {
            action.edge.u >= action.edge.v
                || action.edge.v >= vertex_count
                || action.cost == 0
                || action.states.is_empty()
        })
    {
        return Err(ProofError::new("synthesis action list is not canonical"));
    }
    let max_edits = reader.usize()?;
    let oracle_limit = reader.usize()?;
    let node_limit = reader.usize()?;
    if oracle_limit == 0
        || oracle_limit > FORMAT_MAX_ORACLE_CALLS
        || node_limit == 0
        || node_limit > FORMAT_MAX_SEARCH_NODES
    {
        return Err(ProofError::new(
            "synthesis edit or producer work limit is invalid",
        ));
    }
    let status = VerifiedSynthesisStatus::from_code(reader.u8()?)?;
    let selected = decode_indices(&mut reader, action_count, action_count)?;
    if selected.len() > max_edits {
        return Err(ProofError::new(
            "synthesis selection exceeds the edit limit",
        ));
    }
    let lower_bound = reader.optional_u64()?;
    let upper_bound = reader.optional_u64()?;
    let producer_oracle_calls = reader.usize()?;
    let producer_search_nodes = reader.usize()?;
    let _producer_cache_hits = reader.usize()?;
    if producer_oracle_calls > oracle_limit || producer_search_nodes > node_limit {
        return Err(ProofError::new(
            "synthesis producer work exceeds its declared limit",
        ));
    }
    let root_count = reader.bounded_usize("root blocker count", limits.max_terms)?;
    let mut root_blockers = Vec::with_capacity(root_count);
    let mut proof_terms = 0usize;
    for _ in 0..root_count {
        let blocker = decode_indices(&mut reader, action_count, limits.max_terms)?;
        add_terms(&mut proof_terms, blocker.len(), limits)?;
        root_blockers.push(blocker);
    }
    let before_ranks = decode_usizes(&mut reader, state_count)?;
    let after_ranks = decode_usizes(&mut reader, state_count)?;
    let mut decoded_nodes = 0usize;
    let proof = match reader.u8()? {
        0 => None,
        1 => Some(decode_proof(
            &mut reader,
            action_count,
            0,
            &mut decoded_nodes,
            &mut proof_terms,
            limits,
        )?),
        _ => {
            return Err(ProofError::new("synthesis proof-presence flag is invalid"));
        }
    };
    let proof_nodes = reader.usize()?;
    let proof_topology_checks = reader.usize()?;
    if reader.array32()? != expected || reader.remaining() != 0 {
        return Err(ProofError::new(
            "synthesis artifact has a wrong digest or trailing bytes",
        ));
    }
    if proof_nodes != decoded_nodes {
        return Err(ProofError::new(
            "synthesis proof node count differs from its tree",
        ));
    }
    let claim = Claim {
        vertex_count,
        dimension,
        scale,
        modulus,
        source,
        states,
        actions,
        max_edits,
        status,
        selected,
        lower_bound,
        upper_bound,
        root_blockers,
        before_ranks,
        after_ranks,
        proof,
    };
    validate_source(&claim, limits)?;
    let oracle = Oracle::build(&claim, limits)?;
    let checked_before = oracle.target_ranks();
    let checked_after = oracle.intersection_ranks(&claim.selected)?;
    if claim.before_ranks != checked_before || claim.after_ranks != checked_after {
        return Err(ProofError::new(
            "synthesis rank claims differ from exact restriction images",
        ));
    }
    let costs = claim
        .actions
        .iter()
        .map(|action| action.cost)
        .collect::<Vec<_>>();
    let selected_cost = selected_cost(&costs, &claim.selected)?;
    let selected_feasible = !oracle.survives(&claim.selected)?;
    verify_root_blockers(&oracle, &claim.root_blockers, &costs, claim.lower_bound)?;
    let mut verifier =
        TreeVerifier::new(&costs, claim.max_edits, claim.upper_bound, &oracle, limits);
    match claim.status {
        VerifiedSynthesisStatus::Optimal => {
            if !selected_feasible
                || claim.lower_bound != Some(selected_cost)
                || claim.upper_bound != Some(selected_cost)
            {
                return Err(ProofError::new(
                    "optimal synthesis result has an invalid incumbent or bound",
                ));
            }
            verifier.verify_root(
                claim
                    .proof
                    .as_ref()
                    .ok_or_else(|| ProofError::new("optimal synthesis result has no proof tree"))?,
            )?;
        }
        VerifiedSynthesisStatus::Infeasible => {
            if !claim.selected.is_empty()
                || claim.lower_bound.is_some()
                || claim.upper_bound.is_some()
                || selected_feasible
            {
                return Err(ProofError::new(
                    "infeasible synthesis result has an incumbent or finite bound",
                ));
            }
            verifier.verify_root(claim.proof.as_ref().ok_or_else(|| {
                ProofError::new("infeasible synthesis result has no proof tree")
            })?)?;
        }
        VerifiedSynthesisStatus::SearchIncomplete => {
            if claim.proof.is_some()
                || claim.upper_bound.is_some() != selected_feasible
                || claim.upper_bound.is_some_and(|cost| cost != selected_cost)
                || claim
                    .lower_bound
                    .zip(claim.upper_bound)
                    .is_some_and(|(lower, upper)| lower > upper)
            {
                return Err(ProofError::new(
                    "incomplete synthesis result has an invalid gap",
                ));
            }
        }
    }
    if verifier.nodes != proof_nodes || verifier.checks != proof_topology_checks {
        return Err(ProofError::new(
            "synthesis proof work differs from the checked tree",
        ));
    }
    Ok(VerifiedSynthesis {
        dimension,
        modulus,
        source: match &claim.source {
            Source::Finite => VerifiedSynthesisSource::Finite,
            Source::Affine { .. } => VerifiedSynthesisSource::Affine,
        },
        states: state_count,
        actions: action_count,
        status,
        selected: claim.selected.len(),
        total_cost: selected_feasible.then_some(selected_cost),
        lower_bound_cost: lower_bound,
        upper_bound_cost: upper_bound,
        producer_oracle_calls,
        producer_search_nodes,
        proof_nodes,
        proof_topology_checks,
        before_ranks: claim.before_ranks.clone(),
        after_ranks: claim.after_ranks.clone(),
    })
}

#[derive(Clone, PartialEq, Eq)]
struct State {
    scenario: u64,
    step: u64,
    edges: Vec<Edge>,
    target_space: [u8; 32],
    target: Vec<Vec<MapTerm>>,
    max_surviving_rank: usize,
}

enum Source {
    Finite,
    Affine {
        scenario: u64,
        edges: Vec<AffineEdge>,
        start: f64,
        end: f64,
        maximum_rank: usize,
    },
}

#[derive(Clone)]
struct AffineEdge {
    edge: Edge,
    intercept: f64,
    velocity: f64,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct Action {
    edge: Edge,
    cost: u64,
    states: Vec<usize>,
}

struct Claim {
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    source: Source,
    states: Vec<State>,
    actions: Vec<Action>,
    max_edits: usize,
    status: VerifiedSynthesisStatus,
    selected: Vec<usize>,
    lower_bound: Option<u64>,
    upper_bound: Option<u64>,
    root_blockers: Vec<Vec<usize>>,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
    proof: Option<ProofNode>,
}

fn decode_source(reader: &mut Reader<'_>, limits: ProofLimits) -> Result<Source, ProofError> {
    match reader.u8()? {
        0 => Ok(Source::Finite),
        1 => {
            let scenario = reader.u64()?;
            let start = f64::from_bits(reader.u64()?);
            let end = f64::from_bits(reader.u64()?);
            let maximum_rank = reader.usize()?;
            let count = reader.bounded_usize("affine edge count", limits.max_edges)?;
            if count > reader.remaining() / 32 {
                return Err(ProofError::new(
                    "synthesis affine edge count exceeds the remaining bytes",
                ));
            }
            let mut edges = Vec::with_capacity(count);
            for _ in 0..count {
                edges.push(AffineEdge {
                    edge: Edge {
                        u: reader.usize()?,
                        v: reader.usize()?,
                    },
                    intercept: f64::from_bits(reader.u64()?),
                    velocity: f64::from_bits(reader.u64()?),
                });
            }
            Ok(Source::Affine {
                scenario,
                edges,
                start,
                end,
                maximum_rank,
            })
        }
        _ => Err(ProofError::new("synthesis source kind is invalid")),
    }
}

fn validate_source(claim: &Claim, limits: ProofLimits) -> Result<(), ProofError> {
    let Source::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    } = &claim.source
    else {
        return Ok(());
    };
    validate_affine(claim.vertex_count, edges, *start, *end)?;
    let graphs = complete_threshold_graphs(edges, *start, *end, claim.scale, limits)?;
    let mut expected = Vec::new();
    for (step, graph) in graphs.into_iter().enumerate() {
        let space = Space::build(
            claim.vertex_count,
            claim.dimension,
            &graph,
            claim.modulus,
            limits,
        )?;
        if space.rank() <= *maximum_rank {
            continue;
        }
        expected.push(State {
            scenario: *scenario,
            step: step as u64,
            target_space: space.id(
                claim.vertex_count,
                claim.dimension,
                claim.scale,
                claim.modulus,
                &graph,
            ),
            target: (0..space.rank())
                .map(|target| {
                    vec![MapTerm {
                        target,
                        coefficient: 1,
                    }]
                })
                .collect(),
            edges: graph,
            max_surviving_rank: *maximum_rank,
        });
    }
    if expected != claim.states {
        return Err(ProofError::new(
            "synthesis states are not the complete affine threshold schedule",
        ));
    }
    Ok(())
}

fn validate_affine(
    vertex_count: usize,
    edges: &[AffineEdge],
    start: f64,
    end: f64,
) -> Result<(), ProofError> {
    if !start.is_finite() || !end.is_finite() || start >= end {
        return Err(ProofError::new("synthesis affine interval is invalid"));
    }
    let mut previous = None;
    for trajectory in edges {
        if trajectory.edge.u >= trajectory.edge.v
            || trajectory.edge.v >= vertex_count
            || !trajectory.intercept.is_finite()
            || !trajectory.velocity.is_finite()
            || previous.is_some_and(|edge| edge >= trajectory.edge)
        {
            return Err(ProofError::new(
                "synthesis affine edge trajectory is not canonical",
            ));
        }
        for time in [start, end] {
            let weight = trajectory.intercept + trajectory.velocity * time;
            if !weight.is_finite() || weight < 0.0 {
                return Err(ProofError::new(
                    "synthesis affine edge weight leaves its valid range",
                ));
            }
        }
        previous = Some(trajectory.edge);
    }
    Ok(())
}

fn complete_threshold_graphs(
    edges: &[AffineEdge],
    start: f64,
    end: f64,
    scale: f64,
    limits: ProofLimits,
) -> Result<Vec<Vec<Edge>>, ProofError> {
    let start = rational(start);
    let end = rational(end);
    let scale = rational(scale);
    let mut events = BTreeSet::new();
    for edge in edges {
        let velocity = rational(edge.velocity);
        if velocity == BigRational::from_integer(0.into()) {
            continue;
        }
        let time = (&scale - rational(edge.intercept)) / velocity;
        if start < time && time < end {
            events.insert(time);
        }
    }
    let events = events.into_iter().collect::<Vec<_>>();
    let graph_count = events
        .len()
        .checked_mul(2)
        .and_then(|count| count.checked_add(3))
        .ok_or_else(|| ProofError::new("synthesis affine state count overflows"))?;
    if graph_count > limits.max_snapshots.min(FORMAT_MAX_STATES) {
        return Err(ProofError::new(
            "synthesis affine schedule exceeds its state limit",
        ));
    }
    let mut graphs = Vec::with_capacity(graph_count);
    graphs.push(active_edges(edges, &start, &scale));
    for position in 0..=events.len() {
        let left = if position == 0 {
            &start
        } else {
            &events[position - 1]
        };
        let right = events.get(position).unwrap_or(&end);
        graphs.push(active_edges(edges, &midpoint(left, right), &scale));
        if let Some(time) = events.get(position) {
            graphs.push(active_edges(edges, time, &scale));
        }
    }
    graphs.push(active_edges(edges, &end, &scale));
    Ok(graphs)
}

fn active_edges(edges: &[AffineEdge], time: &BigRational, scale: &BigRational) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| rational(edge.intercept) + rational(edge.velocity) * time <= *scale)
        .map(|edge| edge.edge)
        .collect()
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

fn midpoint(left: &BigRational, right: &BigRational) -> BigRational {
    (left + right) / BigRational::from_integer(2.into())
}

struct Oracle<'a> {
    claim: &'a Claim,
    spaces: Vec<Space>,
    targets: Vec<Vec<Vec<MapTerm>>>,
    limits: ProofLimits,
    cache: RefCell<BTreeMap<(usize, Vec<usize>), usize>>,
}

impl<'a> Oracle<'a> {
    fn build(claim: &'a Claim, limits: ProofLimits) -> Result<Self, ProofError> {
        let mut spaces = Vec::with_capacity(claim.states.len());
        let mut targets = Vec::with_capacity(claim.states.len());
        for state in &claim.states {
            let space = Space::build(
                claim.vertex_count,
                claim.dimension,
                &state.edges,
                claim.modulus,
                limits,
            )?;
            if space.id(
                claim.vertex_count,
                claim.dimension,
                claim.scale,
                claim.modulus,
                &state.edges,
            ) != state.target_space
            {
                return Err(ProofError::new(
                    "synthesis target is bound to a different active complex",
                ));
            }
            let target = space.canonical_subspace(&state.target, claim.modulus)?;
            if target != state.target
                || target.is_empty()
                || state.max_surviving_rank >= target.len()
            {
                return Err(ProofError::new(
                    "synthesis target is not a canonical constrained subspace",
                ));
            }
            spaces.push(space);
            targets.push(target);
        }
        Ok(Self {
            claim,
            spaces,
            targets,
            limits,
            cache: RefCell::new(BTreeMap::new()),
        })
    }

    fn target_ranks(&self) -> Vec<usize> {
        self.targets.iter().map(Vec::len).collect()
    }

    fn survives(&self, selected: &[usize]) -> Result<bool, ProofError> {
        Ok(self
            .intersection_ranks(selected)?
            .iter()
            .zip(&self.claim.states)
            .any(|(rank, state)| *rank > state.max_surviving_rank))
    }

    fn intersection_ranks(&self, selected: &[usize]) -> Result<Vec<usize>, ProofError> {
        (0..self.claim.states.len())
            .map(|state| self.intersection_rank(state, selected))
            .collect()
    }

    fn intersection_rank(&self, state: usize, selected: &[usize]) -> Result<usize, ProofError> {
        let relevant = selected
            .iter()
            .copied()
            .filter(|action| {
                self.claim.actions[*action]
                    .states
                    .binary_search(&state)
                    .is_ok()
            })
            .collect::<Vec<_>>();
        let key = (state, relevant.clone());
        if let Some(rank) = self.cache.borrow().get(&key) {
            return Ok(*rank);
        }
        let mut edges = self.claim.states[state].edges.clone();
        for action in relevant {
            edges.push(self.claim.actions[action].edge);
        }
        edges.sort();
        edges.dedup();
        let source = Space::build(
            self.claim.vertex_count,
            self.claim.dimension,
            &edges,
            self.claim.modulus,
            self.limits,
        )?;
        let rank = self.spaces[state].subspace_intersection_rank_from(
            &source,
            &self.targets[state],
            self.claim.modulus,
        )?;
        self.cache.borrow_mut().insert(key, rank);
        Ok(rank)
    }
}

#[derive(Clone, Copy)]
enum BoundKind {
    Cost,
    Edits,
}

enum ProofNode {
    Cost,
    SurvivingMaximum,
    SurvivingEditLimit,
    BlockerBound {
        kind: BoundKind,
        blockers: Vec<Vec<usize>>,
    },
    Branch {
        blocker: Vec<usize>,
        children: Vec<ProofNode>,
    },
}

struct TreeVerifier<'a> {
    costs: &'a [u64],
    max_edits: usize,
    cutoff: Option<u64>,
    oracle: &'a Oracle<'a>,
    limits: ProofLimits,
    nodes: usize,
    checks: usize,
    terms: usize,
}

impl<'a> TreeVerifier<'a> {
    fn new(
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

    fn verify_root(&mut self, proof: &ProofNode) -> Result<(), ProofError> {
        self.verify_node(proof, Vec::new(), (0..self.costs.len()).collect(), 0)
    }

    fn verify_node(
        &mut self,
        proof: &ProofNode,
        included: Vec<usize>,
        available: Vec<usize>,
        depth: usize,
    ) -> Result<(), ProofError> {
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
        let included_cost = selected_cost(self.costs, &included)?;
        match proof {
            ProofNode::Cost => {
                if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
                    return Err(ProofError::new(
                        "synthesis cost leaf does not reach the incumbent",
                    ));
                }
            }
            ProofNode::SurvivingMaximum => {
                if !self.check_survival(&merge(&included, &available))? {
                    return Err(ProofError::new(
                        "synthesis maximal-survival leaf is feasible",
                    ));
                }
            }
            ProofNode::SurvivingEditLimit => {
                if included.len() != self.max_edits || !self.check_survival(&included)? {
                    return Err(ProofError::new("synthesis edit-limit leaf is invalid"));
                }
            }
            ProofNode::BlockerBound { kind, blockers } => {
                self.verify_blockers(&included, &available, blockers)?;
                match kind {
                    BoundKind::Edits
                        if included.len().saturating_add(blockers.len()) > self.max_edits => {}
                    BoundKind::Cost
                        if self.cutoff.is_some_and(|cutoff| {
                            included_cost
                                .checked_add(
                                    blocker_bound(self.costs, blockers).unwrap_or(u64::MAX),
                                )
                                .is_some_and(|bound| bound >= cutoff)
                        }) => {}
                    _ => {
                        return Err(ProofError::new(
                            "synthesis blocker leaf does not close its branch",
                        ));
                    }
                }
            }
            ProofNode::Branch { blocker, children } => {
                self.verify_blockers(&included, &available, std::slice::from_ref(blocker))?;
                if blocker.len() != children.len() {
                    return Err(ProofError::new(
                        "synthesis branch child count differs from its blocker",
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

fn verify_root_blockers(
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
    *nodes = nodes
        .checked_add(1)
        .ok_or_else(|| ProofError::new("synthesis proof node count overflows"))?;
    if *nodes > limits.max_nodes.min(FORMAT_MAX_PROOF_NODES) || depth > FORMAT_MAX_PROOF_DEPTH {
        return Err(ProofError::new(
            "synthesis proof tree exceeds its node or depth limit",
        ));
    }
    match reader.u8()? {
        1 => Ok(ProofNode::Cost),
        2 => Ok(ProofNode::SurvivingMaximum),
        3 => Ok(ProofNode::SurvivingEditLimit),
        4 => {
            let kind = match reader.u8()? {
                1 => BoundKind::Cost,
                2 => BoundKind::Edits,
                _ => {
                    return Err(ProofError::new("synthesis proof bound kind is invalid"));
                }
            };
            let count = reader.bounded_usize("proof blocker count", limits.max_terms)?;
            let mut blockers = Vec::with_capacity(count);
            for _ in 0..count {
                let blocker = decode_indices(reader, action_count, limits.max_terms)?;
                add_terms(terms, blocker.len(), limits)?;
                blockers.push(blocker);
            }
            Ok(ProofNode::BlockerBound { kind, blockers })
        }
        5 => {
            let blocker = decode_indices(reader, action_count, limits.max_terms)?;
            add_terms(terms, blocker.len(), limits)?;
            let child_count = reader.bounded_usize("proof child count", action_count)?;
            if child_count != blocker.len() {
                return Err(ProofError::new(
                    "synthesis branch child count differs from its blocker",
                ));
            }
            let mut children = Vec::with_capacity(child_count);
            for _ in 0..child_count {
                children.push(decode_proof(
                    reader,
                    action_count,
                    depth + 1,
                    nodes,
                    terms,
                    limits,
                )?);
            }
            Ok(ProofNode::Branch { blocker, children })
        }
        _ => Err(ProofError::new("synthesis proof node kind is invalid")),
    }
}

fn add_terms(total: &mut usize, count: usize, limits: ProofLimits) -> Result<(), ProofError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| ProofError::new("synthesis proof term count overflows"))?;
    if *total > limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
        return Err(ProofError::new("synthesis proof terms exceed their limit"));
    }
    Ok(())
}

fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64, ProofError> {
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

fn blocker_bound(costs: &[u64], blockers: &[Vec<usize>]) -> Result<u64, ProofError> {
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

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum: usize,
) -> Result<Vec<Edge>, ProofError> {
    let count = reader.bounded_usize("edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(ProofError::new(
            "synthesis edge count exceeds the remaining bytes",
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
            "synthesis active edge list is not canonical",
        ));
    }
    Ok(edges)
}

fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("integer list count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(
            "synthesis integer list exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

fn decode_indices(
    reader: &mut Reader<'_>,
    exclusive_maximum: usize,
    maximum_count: usize,
) -> Result<Vec<usize>, ProofError> {
    let values = decode_usizes(reader, maximum_count)?;
    if values.iter().any(|value| *value >= exclusive_maximum)
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new("synthesis index list is not canonical"));
    }
    Ok(values)
}

fn is_prime(value: u64) -> bool {
    if value < 2 {
        return false;
    }
    if value % 2 == 0 {
        return value == 2;
    }
    let mut divisor = 3u64;
    while divisor <= value / divisor {
        if value % divisor == 0 {
            return false;
        }
        divisor += 2;
    }
    true
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], ProofError> {
        let end = self
            .position
            .checked_add(count)
            .ok_or_else(|| ProofError::new("synthesis artifact position overflows"))?;
        if end > self.bytes.len() {
            return Err(ProofError::new("synthesis artifact is truncated"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > maximum {
            return Err(ProofError::new(format!(
                "synthesis {name} exceeds its limit"
            )));
        }
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, ProofError> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, ProofError> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }

    fn u32(&mut self) -> Result<u32, ProofError> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }

    fn u64(&mut self) -> Result<u64, ProofError> {
        Ok(u64::from_be_bytes(self.take(8)?.try_into().unwrap()))
    }

    fn usize(&mut self) -> Result<usize, ProofError> {
        usize::try_from(self.u64()?)
            .map_err(|_| ProofError::new("synthesis integer does not fit usize"))
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(ProofError::new(
                "synthesis optional integer flag is invalid",
            )),
        }
    }

    fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}
