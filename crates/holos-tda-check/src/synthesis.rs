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
    let decoded = decode_synthesis(bytes, limits)?;
    let checked = verify_claim(&decoded, limits)?;
    Ok(synthesis_summary(&decoded, checked))
}

struct DecodedSynthesis {
    claim: Claim,
    producer_oracle_calls: usize,
    producer_search_nodes: usize,
    proof_nodes: usize,
    proof_topology_checks: usize,
}

struct SynthesisHeader {
    vertex_count: usize,
    dimension: usize,
    scale: f64,
    modulus: u32,
    source: Source,
}

struct WorkLimits {
    oracle: usize,
    nodes: usize,
}

struct Selection {
    status: VerifiedSynthesisStatus,
    selected: Vec<usize>,
    lower_bound: Option<u64>,
    upper_bound: Option<u64>,
}

struct SearchData {
    max_edits: usize,
    selection: Selection,
    producer_oracle_calls: usize,
    producer_search_nodes: usize,
}

struct ProofData {
    root_blockers: Vec<Vec<usize>>,
    before_ranks: Vec<usize>,
    after_ranks: Vec<usize>,
    proof: Option<ProofNode>,
    nodes: usize,
    topology_checks: usize,
}

struct CheckedSynthesis {
    selected_cost: u64,
    selected_feasible: bool,
}

fn decode_synthesis(bytes: &[u8], limits: ProofLimits) -> Result<DecodedSynthesis, ProofError> {
    let expected = expected_digest(bytes, limits)?;
    let mut reader = Reader::new(bytes);
    decode_prefix(&mut reader)?;
    let header = decode_header(&mut reader, limits)?;
    let states = decode_states(&mut reader, header.vertex_count, limits)?;
    let actions = decode_actions(&mut reader, header.vertex_count, states.len(), limits)?;
    let search = decode_search(&mut reader, actions.len())?;
    let proof = decode_proof_data(&mut reader, actions.len(), states.len(), limits)?;
    decode_trailer(&mut reader, expected)?;
    let claim = Claim {
        vertex_count: header.vertex_count,
        dimension: header.dimension,
        scale: header.scale,
        modulus: header.modulus,
        source: header.source,
        states,
        actions,
        max_edits: search.max_edits,
        status: search.selection.status,
        selected: search.selection.selected,
        lower_bound: search.selection.lower_bound,
        upper_bound: search.selection.upper_bound,
        root_blockers: proof.root_blockers,
        before_ranks: proof.before_ranks,
        after_ranks: proof.after_ranks,
        proof: proof.proof,
    };
    Ok(DecodedSynthesis {
        claim,
        producer_oracle_calls: search.producer_oracle_calls,
        producer_search_nodes: search.producer_search_nodes,
        proof_nodes: proof.nodes,
        proof_topology_checks: proof.topology_checks,
    })
}

fn expected_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "synthesis artifact exceeds its byte limit or is truncated",
        ));
    }
    let mut hash = Sha256::new();
    hash.update(b"holos-synthesis-artifact-v1");
    hash.update(&bytes[..bytes.len() - 32]);
    Ok(hash.finalize().into())
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported synthesis artifact"));
    }
    Ok(())
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<SynthesisHeader, ProofError> {
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let dimension = reader.bounded_usize("dimension", limits.max_dimension)?;
    let scale = f64::from_bits(reader.u64()?);
    let modulus = reader.u32()?;
    validate_field(scale, modulus)?;
    Ok(SynthesisHeader {
        vertex_count,
        dimension,
        scale,
        modulus,
        source: decode_source(reader, limits)?,
    })
}

fn validate_field(scale: f64, modulus: u32) -> Result<(), ProofError> {
    if !scale.is_finite()
        || scale < 0.0
        || !is_prime(u64::from(modulus))
        || u64::from(modulus) >= MODULUS_LIMIT
    {
        Err(ProofError::new(
            "synthesis scale or coefficient field is invalid",
        ))
    } else {
        Ok(())
    }
}

fn decode_states(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    limits: ProofLimits,
) -> Result<Vec<State>, ProofError> {
    let count = reader.bounded_usize("state count", limits.max_snapshots.min(FORMAT_MAX_STATES))?;
    let mut states = Vec::with_capacity(count);
    let mut edge_count = 0usize;
    let mut target_terms = 0usize;
    let mut prior = None;
    for _ in 0..count {
        let state = decode_state(reader, vertex_count, &mut target_terms, limits)?;
        validate_state_order(prior, &state)?;
        prior = Some((state.scenario, state.step));
        edge_count = add_edge_count(edge_count, state.edges.len(), limits)?;
        states.push(state);
    }
    Ok(states)
}

fn decode_state(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    target_terms: &mut usize,
    limits: ProofLimits,
) -> Result<State, ProofError> {
    let scenario = reader.u64()?;
    let step = reader.u64()?;
    let edges = decode_edges(reader, vertex_count, limits.max_edges)?;
    let target_space = reader.array32()?;
    let target = decode_target(reader, target_terms, limits)?;
    Ok(State {
        scenario,
        step,
        edges,
        target_space,
        target,
        max_surviving_rank: reader.usize()?,
    })
}

fn decode_target(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<MapTerm>>, ProofError> {
    let count = reader.bounded_usize("target row count", limits.max_terms)?;
    let mut target = Vec::with_capacity(count);
    for _ in 0..count {
        target.push(decode_target_row(reader, target_terms, limits)?);
    }
    Ok(target)
}

fn decode_target_row(
    reader: &mut Reader<'_>,
    target_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<MapTerm>, ProofError> {
    let count = reader.bounded_usize("target row term count", limits.max_terms)?;
    *target_terms = target_terms
        .checked_add(count)
        .ok_or_else(|| ProofError::new("synthesis target term count overflows"))?;
    if *target_terms > limits.max_terms || count > reader.remaining() / 12 {
        return Err(ProofError::new(
            "synthesis target terms exceed their limit or remaining bytes",
        ));
    }
    (0..count)
        .map(|_| {
            Ok(MapTerm {
                target: reader.usize()?,
                coefficient: reader.u32()?,
            })
        })
        .collect()
}

fn validate_state_order(prior: Option<(u64, u64)>, state: &State) -> Result<(), ProofError> {
    if prior.is_some_and(|key| key >= (state.scenario, state.step)) {
        Err(ProofError::new(
            "synthesis states are not in canonical scenario and step order",
        ))
    } else {
        Ok(())
    }
}

fn add_edge_count(total: usize, add: usize, limits: ProofLimits) -> Result<usize, ProofError> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| ProofError::new("synthesis edge count overflows"))?;
    if total > limits.max_edges {
        Err(ProofError::new(
            "synthesis state edges exceed their total limit",
        ))
    } else {
        Ok(total)
    }
}

fn decode_actions(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    state_count: usize,
    limits: ProofLimits,
) -> Result<Vec<Action>, ProofError> {
    let count = reader.bounded_usize(
        "action count",
        limits.max_references.min(FORMAT_MAX_ACTIONS),
    )?;
    let mut actions = Vec::with_capacity(count);
    for _ in 0..count {
        actions.push(decode_action(reader, state_count)?);
    }
    validate_actions(&actions, vertex_count)?;
    Ok(actions)
}

fn decode_action(reader: &mut Reader<'_>, state_count: usize) -> Result<Action, ProofError> {
    Ok(Action {
        edge: Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        cost: reader.u64()?,
        states: decode_indices(reader, state_count, state_count)?,
    })
}

fn validate_actions(actions: &[Action], vertex_count: usize) -> Result<(), ProofError> {
    let unordered = actions.windows(2).any(|pair| pair[0] >= pair[1]);
    let invalid = actions.iter().any(|action| {
        action.edge.u >= action.edge.v
            || action.edge.v >= vertex_count
            || action.cost == 0
            || action.states.is_empty()
    });
    if unordered || invalid {
        Err(ProofError::new("synthesis action list is not canonical"))
    } else {
        Ok(())
    }
}

fn decode_search(reader: &mut Reader<'_>, action_count: usize) -> Result<SearchData, ProofError> {
    let max_edits = reader.usize()?;
    let limits = decode_work_limits(reader)?;
    let selection = decode_selection(reader, action_count, max_edits)?;
    let producer_oracle_calls = reader.usize()?;
    let producer_search_nodes = reader.usize()?;
    let _producer_cache_hits = reader.usize()?;
    if producer_oracle_calls > limits.oracle || producer_search_nodes > limits.nodes {
        return Err(ProofError::new(
            "synthesis producer work exceeds its declared limit",
        ));
    }
    Ok(SearchData {
        max_edits,
        selection,
        producer_oracle_calls,
        producer_search_nodes,
    })
}

fn decode_work_limits(reader: &mut Reader<'_>) -> Result<WorkLimits, ProofError> {
    let oracle = reader.usize()?;
    let nodes = reader.usize()?;
    if oracle == 0
        || oracle > FORMAT_MAX_ORACLE_CALLS
        || nodes == 0
        || nodes > FORMAT_MAX_SEARCH_NODES
    {
        Err(ProofError::new(
            "synthesis edit or producer work limit is invalid",
        ))
    } else {
        Ok(WorkLimits { oracle, nodes })
    }
}

fn decode_selection(
    reader: &mut Reader<'_>,
    action_count: usize,
    max_edits: usize,
) -> Result<Selection, ProofError> {
    let status = VerifiedSynthesisStatus::from_code(reader.u8()?)?;
    let selected = decode_indices(reader, action_count, action_count)?;
    if selected.len() > max_edits {
        return Err(ProofError::new(
            "synthesis selection exceeds the edit limit",
        ));
    }
    Ok(Selection {
        status,
        selected,
        lower_bound: reader.optional_u64()?,
        upper_bound: reader.optional_u64()?,
    })
}

fn decode_proof_data(
    reader: &mut Reader<'_>,
    action_count: usize,
    state_count: usize,
    limits: ProofLimits,
) -> Result<ProofData, ProofError> {
    let mut terms = 0usize;
    let root_blockers = decode_root_blockers(reader, action_count, &mut terms, limits)?;
    let before_ranks = decode_usizes(reader, state_count)?;
    let after_ranks = decode_usizes(reader, state_count)?;
    let mut nodes = 0usize;
    let proof = decode_optional_proof(reader, action_count, &mut nodes, &mut terms, limits)?;
    let claimed_nodes = reader.usize()?;
    let topology_checks = reader.usize()?;
    if claimed_nodes != nodes {
        return Err(ProofError::new(
            "synthesis proof node count differs from its tree",
        ));
    }
    Ok(ProofData {
        root_blockers,
        before_ranks,
        after_ranks,
        proof,
        nodes,
        topology_checks,
    })
}

fn decode_root_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    terms: &mut usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<usize>>, ProofError> {
    let count = reader.bounded_usize("root blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_terms(terms, blocker.len(), limits)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn decode_optional_proof(
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

fn decode_trailer(reader: &mut Reader<'_>, expected: [u8; 32]) -> Result<(), ProofError> {
    if reader.array32()? != expected || reader.remaining() != 0 {
        Err(ProofError::new(
            "synthesis artifact has a wrong digest or trailing bytes",
        ))
    } else {
        Ok(())
    }
}

fn verify_claim(
    decoded: &DecodedSynthesis,
    limits: ProofLimits,
) -> Result<CheckedSynthesis, ProofError> {
    let claim = &decoded.claim;
    validate_source(claim, limits)?;
    let oracle = Oracle::build(claim, limits)?;
    verify_rank_claims(claim, &oracle)?;
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
    verify_status(claim, selected_cost, selected_feasible, &mut verifier)?;
    verify_proof_work(decoded, &verifier)?;
    Ok(CheckedSynthesis {
        selected_cost,
        selected_feasible,
    })
}

fn verify_rank_claims(claim: &Claim, oracle: &Oracle<'_>) -> Result<(), ProofError> {
    let before = oracle.target_ranks();
    let after = oracle.intersection_ranks(&claim.selected)?;
    if claim.before_ranks != before || claim.after_ranks != after {
        Err(ProofError::new(
            "synthesis rank claims differ from exact restriction images",
        ))
    } else {
        Ok(())
    }
}

fn verify_status(
    claim: &Claim,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    match claim.status {
        VerifiedSynthesisStatus::Optimal => {
            verify_optimal(claim, selected_cost, selected_feasible, verifier)
        }
        VerifiedSynthesisStatus::Infeasible => {
            verify_infeasible(claim, selected_feasible, verifier)
        }
        VerifiedSynthesisStatus::SearchIncomplete => {
            verify_incomplete(claim, selected_cost, selected_feasible)
        }
    }
}

fn verify_optimal(
    claim: &Claim,
    selected_cost: u64,
    selected_feasible: bool,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !selected_feasible
        || claim.lower_bound != Some(selected_cost)
        || claim.upper_bound != Some(selected_cost)
    {
        return Err(ProofError::new(
            "optimal synthesis result has an invalid incumbent or bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("optimal synthesis result has no proof tree"))?;
    verifier.verify_root(proof)
}

fn verify_infeasible(
    claim: &Claim,
    selected_feasible: bool,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !claim.selected.is_empty()
        || claim.lower_bound.is_some()
        || claim.upper_bound.is_some()
        || selected_feasible
    {
        return Err(ProofError::new(
            "infeasible synthesis result has an incumbent or finite bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("infeasible synthesis result has no proof tree"))?;
    verifier.verify_root(proof)
}

fn verify_incomplete(
    claim: &Claim,
    selected_cost: u64,
    selected_feasible: bool,
) -> Result<(), ProofError> {
    if claim.proof.is_some()
        || claim.upper_bound.is_some() != selected_feasible
        || claim.upper_bound.is_some_and(|cost| cost != selected_cost)
        || claim
            .lower_bound
            .zip(claim.upper_bound)
            .is_some_and(|(lower, upper)| lower > upper)
    {
        Err(ProofError::new(
            "incomplete synthesis result has an invalid gap",
        ))
    } else {
        Ok(())
    }
}

fn verify_proof_work(
    decoded: &DecodedSynthesis,
    verifier: &TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if verifier.nodes != decoded.proof_nodes || verifier.checks != decoded.proof_topology_checks {
        Err(ProofError::new(
            "synthesis proof work differs from the checked tree",
        ))
    } else {
        Ok(())
    }
}

fn synthesis_summary(decoded: &DecodedSynthesis, checked: CheckedSynthesis) -> VerifiedSynthesis {
    let claim = &decoded.claim;
    VerifiedSynthesis {
        dimension: claim.dimension,
        modulus: claim.modulus,
        source: match claim.source {
            Source::Finite => VerifiedSynthesisSource::Finite,
            Source::Affine { .. } => VerifiedSynthesisSource::Affine,
        },
        states: claim.states.len(),
        actions: claim.actions.len(),
        status: claim.status,
        selected: claim.selected.len(),
        total_cost: checked.selected_feasible.then_some(checked.selected_cost),
        lower_bound_cost: claim.lower_bound,
        upper_bound_cost: claim.upper_bound,
        producer_oracle_calls: decoded.producer_oracle_calls,
        producer_search_nodes: decoded.producer_search_nodes,
        proof_nodes: decoded.proof_nodes,
        proof_topology_checks: decoded.proof_topology_checks,
        before_ranks: claim.before_ranks.clone(),
        after_ranks: claim.after_ranks.clone(),
    }
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
        1 => decode_affine_source(reader, limits),
        _ => Err(ProofError::new("synthesis source kind is invalid")),
    }
}

fn decode_affine_source(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Source, ProofError> {
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
        edges.push(decode_affine_edge(reader)?);
    }
    Ok(Source::Affine {
        scenario,
        edges,
        start,
        end,
        maximum_rank,
    })
}

fn decode_affine_edge(reader: &mut Reader<'_>) -> Result<AffineEdge, ProofError> {
    Ok(AffineEdge {
        edge: Edge {
            u: reader.usize()?,
            v: reader.usize()?,
        },
        intercept: f64::from_bits(reader.u64()?),
        velocity: f64::from_bits(reader.u64()?),
    })
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
    validate_affine_interval(start, end)?;
    let mut previous = None;
    for trajectory in edges {
        validate_trajectory(trajectory, previous, vertex_count, start, end)?;
        previous = Some(trajectory.edge);
    }
    Ok(())
}

fn validate_affine_interval(start: f64, end: f64) -> Result<(), ProofError> {
    if !start.is_finite() || !end.is_finite() || start >= end {
        Err(ProofError::new("synthesis affine interval is invalid"))
    } else {
        Ok(())
    }
}

fn validate_trajectory(
    trajectory: &AffineEdge,
    previous: Option<Edge>,
    vertex_count: usize,
    start: f64,
    end: f64,
) -> Result<(), ProofError> {
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
    validate_trajectory_weight(trajectory, start)?;
    validate_trajectory_weight(trajectory, end)
}

fn validate_trajectory_weight(trajectory: &AffineEdge, time: f64) -> Result<(), ProofError> {
    let weight = trajectory.intercept + trajectory.velocity * time;
    if !weight.is_finite() || weight < 0.0 {
        Err(ProofError::new(
            "synthesis affine edge weight leaves its valid range",
        ))
    } else {
        Ok(())
    }
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
