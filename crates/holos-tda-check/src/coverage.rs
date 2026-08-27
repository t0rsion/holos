use std::collections::{BTreeMap, BTreeSet};

use num_rational::BigRational;
use sha2::{Digest, Sha256};

use crate::{ProofError, ProofLimits};

const MAGIC: &[u8; 8] = b"HOLOSCOV";
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

/// Completeness status accepted from a coverage proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedCoverageStatus {
    /// The checked activation set has minimum total cost.
    Optimal,
    /// No activation set within the declared limit satisfies every state.
    Infeasible,
    /// The producer stopped with a checked bound or incumbent.
    SearchIncomplete,
}

impl VerifiedCoverageStatus {
    fn from_code(code: u8) -> Result<Self, ProofError> {
        match code {
            1 => Ok(Self::Optimal),
            2 => Ok(Self::Infeasible),
            3 => Ok(Self::SearchIncomplete),
            _ => Err(ProofError::new("coverage status is invalid")),
        }
    }
}

/// Completeness scope accepted from a coverage proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifiedCoverageSource {
    /// The claim covers only its listed communication states.
    Finite,
    /// The checker reconstructed the complete affine threshold schedule.
    Affine,
}

/// Summary of an independently checked coverage result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedCoverage {
    /// Prime coefficient modulus.
    pub modulus: u32,
    /// Completeness scope of the checked state list.
    pub source: VerifiedCoverageSource,
    /// Number of checked communication states.
    pub states: usize,
    /// Number of declared candidate activations.
    pub actions: usize,
    /// Largest number of simultaneous sensor failures.
    pub failure_budget: usize,
    /// Completeness status.
    pub status: VerifiedCoverageStatus,
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
    /// State-failure pairs checked for the selected plan.
    pub selected_failure_checks: usize,
    /// Smallest selected-plan two-chain support across all failure checks.
    pub minimum_witness_triangles: Option<usize>,
}

/// Return true when bytes start with a coverage envelope.
pub fn is_coverage(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

/// Verify one bounded coverage proof without invoking `holos-tda`.
pub fn verify_coverage(bytes: &[u8], limits: ProofLimits) -> Result<VerifiedCoverage, ProofError> {
    let decoded = decode_coverage(bytes, limits)?;
    let checked = verify_claim(&decoded, limits)?;
    Ok(coverage_summary(&decoded, checked))
}

pub(crate) fn verify_coverage_with_geometry_claim(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<(VerifiedCoverage, CoverageGeometryClaim), ProofError> {
    let decoded = decode_coverage(bytes, limits)?;
    let checked = verify_claim(&decoded, limits)?;
    if !matches!(decoded.claim.source, Source::Finite) {
        return Err(ProofError::new(
            "geometry binding accepts finite coverage states only",
        ));
    }
    let summary = coverage_summary(&decoded, checked);
    let claim = CoverageGeometryClaim {
        vertex_count: decoded.claim.vertex_count,
        broadcast_radius: decoded.claim.broadcast_radius,
        sensing_radius: decoded.claim.sensing_radius,
        fence: decoded.claim.fence.clone(),
        state_edges: decoded
            .claim
            .states
            .iter()
            .map(|state| state.edges.iter().map(|edge| (edge.u, edge.v)).collect())
            .collect(),
    };
    Ok((summary, claim))
}

pub(crate) struct CoverageGeometryClaim {
    pub(crate) vertex_count: usize,
    pub(crate) broadcast_radius: f64,
    pub(crate) sensing_radius: f64,
    pub(crate) fence: Vec<usize>,
    pub(crate) state_edges: Vec<Vec<(usize, usize)>>,
}

struct DecodedCoverage {
    claim: Claim,
    producer_oracle_calls: usize,
    producer_search_nodes: usize,
    proof_nodes: usize,
    proof_topology_checks: usize,
    proof_terms: usize,
}

struct PhysicalHeader {
    vertex_count: usize,
    broadcast_radius: f64,
    sensing_radius: f64,
    modulus: u32,
    fence: Vec<usize>,
}

struct CoverageHeader {
    physical: PhysicalHeader,
    failable: Vec<usize>,
    failure_budget: usize,
    source: Source,
}

struct WorkLimits {
    oracle: usize,
    nodes: usize,
}

struct Selection {
    status: VerifiedCoverageStatus,
    selected: Vec<usize>,
    lower_bound: Option<u64>,
    upper_bound: Option<u64>,
}

struct SearchData {
    max_activations: usize,
    selection: Selection,
    producer_oracle_calls: usize,
    producer_search_nodes: usize,
}

struct ProofData {
    root_blockers: Vec<Vec<usize>>,
    before: Evaluation,
    after: Evaluation,
    proof: Option<ProofNode>,
    nodes: usize,
    topology_checks: usize,
    terms: usize,
}

struct CheckedCoverage {
    after: Evaluation,
    selected_cost: u64,
}

fn decode_coverage(bytes: &[u8], limits: ProofLimits) -> Result<DecodedCoverage, ProofError> {
    let expected = expected_digest(bytes, limits)?;
    let mut reader = Reader::new(bytes);
    decode_prefix(&mut reader)?;
    let header = decode_header(&mut reader, limits)?;
    let states = decode_states(
        &mut reader,
        header.physical.vertex_count,
        &header.physical.fence,
        limits,
    )?;
    let actions = decode_actions(
        &mut reader,
        header.physical.vertex_count,
        &header.physical.fence,
        &states,
        limits,
    )?;
    let search = decode_search(&mut reader, actions.len())?;
    let proof = decode_proof_data(&mut reader, actions.len(), limits)?;
    decode_trailer(&mut reader, expected)?;
    let claim = Claim {
        vertex_count: header.physical.vertex_count,
        broadcast_radius: header.physical.broadcast_radius,
        sensing_radius: header.physical.sensing_radius,
        modulus: header.physical.modulus,
        fence: header.physical.fence,
        failable: header.failable,
        failure_budget: header.failure_budget,
        source: header.source,
        states,
        actions,
        max_activations: search.max_activations,
        status: search.selection.status,
        selected: search.selection.selected,
        lower_bound: search.selection.lower_bound,
        upper_bound: search.selection.upper_bound,
        root_blockers: proof.root_blockers,
        before: proof.before,
        after: proof.after,
        proof: proof.proof,
    };
    Ok(DecodedCoverage {
        claim,
        producer_oracle_calls: search.producer_oracle_calls,
        producer_search_nodes: search.producer_search_nodes,
        proof_nodes: proof.nodes,
        proof_topology_checks: proof.topology_checks,
        proof_terms: proof.terms,
    })
}

fn expected_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "coverage artifact exceeds its byte limit or is truncated",
        ));
    }
    Ok(Sha256::digest(&bytes[..bytes.len() - 32]).into())
}

fn decode_prefix(reader: &mut Reader<'_>) -> Result<(), ProofError> {
    let magic = reader.take(8)?;
    let version = reader.u16()?;
    let codec = reader.u8()?;
    if magic != MAGIC || version != VERSION || codec != F64_BITS_CODEC {
        return Err(ProofError::new("unsupported coverage artifact"));
    }
    Ok(())
}

fn decode_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<CoverageHeader, ProofError> {
    let physical = decode_physical_header(reader, limits)?;
    let failable = decode_indices(reader, physical.vertex_count, limits.max_vertices)?;
    if physical
        .fence
        .iter()
        .any(|vertex| failable.binary_search(vertex).is_ok())
    {
        return Err(ProofError::new("coverage fence vertex is failable"));
    }
    let failure_budget = reader.usize()?;
    if failure_budget > failable.len() {
        return Err(ProofError::new(
            "coverage failure budget exceeds the failable sensor count",
        ));
    }
    let source = decode_source(reader, limits)?;
    Ok(CoverageHeader {
        physical,
        failable,
        failure_budget,
        source,
    })
}

fn decode_physical_header(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<PhysicalHeader, ProofError> {
    let vertex_count = reader.bounded_usize("vertex count", limits.max_vertices)?;
    let broadcast_radius = f64::from_bits(reader.u64()?);
    let sensing_radius = f64::from_bits(reader.u64()?);
    validate_radii(broadcast_radius, sensing_radius)?;
    let modulus = reader.u32()?;
    validate_modulus(modulus)?;
    let fence = decode_usizes(reader, limits.max_vertices)?;
    validate_fence(vertex_count, &fence)?;
    Ok(PhysicalHeader {
        vertex_count,
        broadcast_radius,
        sensing_radius,
        modulus,
        fence,
    })
}

fn validate_modulus(modulus: u32) -> Result<(), ProofError> {
    if !is_prime(u64::from(modulus)) || u64::from(modulus) >= MODULUS_LIMIT {
        return Err(ProofError::new(
            "coverage modulus must be a supported prime",
        ));
    }
    Ok(())
}

fn validate_fence(vertex_count: usize, fence: &[usize]) -> Result<(), ProofError> {
    let distinct = fence.iter().copied().collect::<BTreeSet<_>>().len();
    if fence.len() < 3
        || fence.iter().any(|vertex| *vertex >= vertex_count)
        || distinct != fence.len()
        || canonical_fence(fence) != fence
    {
        return Err(ProofError::new("coverage fence is not a canonical cycle"));
    }
    Ok(())
}

fn decode_states(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    fence: &[usize],
    limits: ProofLimits,
) -> Result<Vec<State>, ProofError> {
    let count = reader.bounded_usize("state count", limits.max_snapshots.min(FORMAT_MAX_STATES))?;
    if vertex_count == 0 || count == 0 {
        return Err(ProofError::new("coverage specification has an empty scope"));
    }
    let mut states = Vec::with_capacity(count);
    let mut total_edges = 0usize;
    let mut prior = None;
    for _ in 0..count {
        let state = decode_state(reader, vertex_count, fence, limits)?;
        validate_state_order(prior, &state)?;
        prior = Some((state.scenario, state.step));
        total_edges = add_edge_count(total_edges, state.edges.len(), limits)?;
        states.push(state);
    }
    Ok(states)
}

fn decode_state(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    fence: &[usize],
    limits: ProofLimits,
) -> Result<State, ProofError> {
    let scenario = reader.u64()?;
    let step = reader.u64()?;
    let base = decode_indices(reader, vertex_count, limits.max_vertices)?;
    if fence
        .iter()
        .any(|vertex| base.binary_search(vertex).is_err())
    {
        return Err(ProofError::new("coverage state omits a fence vertex"));
    }
    let edges = decode_edges(reader, vertex_count, limits.max_edges)?;
    Ok(State {
        scenario,
        step,
        base,
        edges,
    })
}

fn validate_state_order(prior: Option<(u64, u64)>, state: &State) -> Result<(), ProofError> {
    if prior.is_some_and(|key| key >= (state.scenario, state.step)) {
        return Err(ProofError::new(
            "coverage states are not in canonical scenario and step order",
        ));
    }
    Ok(())
}

fn add_edge_count(total: usize, add: usize, limits: ProofLimits) -> Result<usize, ProofError> {
    let total = total
        .checked_add(add)
        .ok_or_else(|| ProofError::new("coverage edge count overflows"))?;
    if total > limits.max_edges {
        return Err(ProofError::new(
            "coverage state edges exceed their total limit",
        ));
    }
    Ok(total)
}

fn decode_actions(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    fence: &[usize],
    states: &[State],
    limits: ProofLimits,
) -> Result<Vec<Action>, ProofError> {
    let count = reader.bounded_usize(
        "action count",
        limits.max_references.min(FORMAT_MAX_ACTIONS),
    )?;
    let mut actions = Vec::with_capacity(count);
    let mut vertices = BTreeSet::new();
    let mut cost_sum = 0u64;
    for _ in 0..count {
        let action = decode_action(reader, vertex_count, states.len())?;
        validate_action(&action, vertex_count, fence, states, &mut vertices)?;
        cost_sum = cost_sum
            .checked_add(action.cost)
            .ok_or_else(|| ProofError::new("coverage action cost sum overflows"))?;
        actions.push(action);
    }
    Ok(actions)
}

fn decode_action(
    reader: &mut Reader<'_>,
    _vertex_count: usize,
    state_count: usize,
) -> Result<Action, ProofError> {
    Ok(Action {
        vertex: reader.usize()?,
        cost: reader.u64()?,
        states: decode_indices(reader, state_count, state_count)?,
    })
}

fn validate_action(
    action: &Action,
    vertex_count: usize,
    fence: &[usize],
    states: &[State],
    vertices: &mut BTreeSet<usize>,
) -> Result<(), ProofError> {
    let already_active = action
        .states
        .iter()
        .any(|state| states[*state].base.binary_search(&action.vertex).is_ok());
    if action.vertex >= vertex_count
        || action.cost == 0
        || action.states.is_empty()
        || fence.contains(&action.vertex)
        || !vertices.insert(action.vertex)
        || already_active
    {
        return Err(ProofError::new(
            "coverage action list is not a canonical activation set",
        ));
    }
    Ok(())
}

fn decode_search(reader: &mut Reader<'_>, action_count: usize) -> Result<SearchData, ProofError> {
    let max_activations = reader.usize()?;
    let limits = decode_work_limits(reader)?;
    let selection = decode_selection(reader, action_count, max_activations)?;
    let producer_oracle_calls = reader.usize()?;
    let producer_search_nodes = reader.usize()?;
    let _producer_cache_hits = reader.usize()?;
    if producer_oracle_calls > limits.oracle || producer_search_nodes > limits.nodes {
        return Err(ProofError::new(
            "coverage producer work exceeds its declared limit",
        ));
    }
    Ok(SearchData {
        max_activations,
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
        return Err(ProofError::new("coverage producer work limit is invalid"));
    }
    Ok(WorkLimits { oracle, nodes })
}

fn decode_selection(
    reader: &mut Reader<'_>,
    action_count: usize,
    max_activations: usize,
) -> Result<Selection, ProofError> {
    let status = VerifiedCoverageStatus::from_code(reader.u8()?)?;
    let selected = decode_indices(reader, action_count, action_count)?;
    if selected.len() > max_activations.min(action_count) {
        return Err(ProofError::new(
            "coverage selection exceeds the activation limit",
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
    limits: ProofLimits,
) -> Result<ProofData, ProofError> {
    let root_blockers = decode_root_blockers(reader, action_count, limits)?;
    let before = decode_evaluation(reader)?;
    let after = decode_evaluation(reader)?;
    let mut decoded_nodes = 0usize;
    let mut decoded_terms = 0usize;
    let proof = decode_optional_proof(
        reader,
        action_count,
        &mut decoded_nodes,
        &mut decoded_terms,
        limits,
    )?;
    let nodes = reader.usize()?;
    let topology_checks = reader.usize()?;
    let terms = reader.usize()?;
    if nodes != decoded_nodes || terms != decoded_terms {
        return Err(ProofError::new("coverage proof size differs from its tree"));
    }
    Ok(ProofData {
        root_blockers,
        before,
        after,
        proof,
        nodes,
        topology_checks,
        terms,
    })
}

fn decode_root_blockers(
    reader: &mut Reader<'_>,
    action_count: usize,
    limits: ProofLimits,
) -> Result<Vec<Vec<usize>>, ProofError> {
    let count = reader.bounded_usize("root blocker count", limits.max_terms)?;
    let mut blockers = Vec::with_capacity(count);
    let mut terms = 0usize;
    for _ in 0..count {
        let blocker = decode_indices(reader, action_count, limits.max_terms)?;
        add_terms(&mut terms, blocker.len(), limits)?;
        blockers.push(blocker);
    }
    Ok(blockers)
}

fn decode_optional_proof(
    reader: &mut Reader<'_>,
    action_count: usize,
    decoded_nodes: &mut usize,
    decoded_terms: &mut usize,
    limits: ProofLimits,
) -> Result<Option<ProofNode>, ProofError> {
    match reader.u8()? {
        0 => Ok(None),
        1 => decode_proof(
            reader,
            action_count,
            0,
            decoded_nodes,
            decoded_terms,
            limits,
        )
        .map(Some),
        _ => Err(ProofError::new("coverage proof-presence flag is invalid")),
    }
}

fn decode_trailer(reader: &mut Reader<'_>, expected: [u8; 32]) -> Result<(), ProofError> {
    if reader.array32()? != expected || reader.remaining() != 0 {
        return Err(ProofError::new(
            "coverage artifact has a wrong digest or trailing bytes",
        ));
    }
    Ok(())
}

fn verify_claim(
    decoded: &DecodedCoverage,
    limits: ProofLimits,
) -> Result<CheckedCoverage, ProofError> {
    let claim = &decoded.claim;
    validate_source(claim, limits)?;
    let checked_before = evaluate(claim, &[], limits)?;
    let checked_after = evaluate(claim, &claim.selected, limits)?;
    if claim.before != checked_before || claim.after != checked_after {
        return Err(ProofError::new(
            "coverage evaluation claims differ from exact checks",
        ));
    }
    let costs = claim
        .actions
        .iter()
        .map(|action| action.cost)
        .collect::<Vec<_>>();
    let selected_cost = selected_cost(&costs, &claim.selected)?;
    verify_root_blockers(
        claim,
        &claim.root_blockers,
        &costs,
        claim.lower_bound,
        limits,
    )?;
    let mut verifier = TreeVerifier::new(
        &costs,
        claim.max_activations.min(claim.actions.len()),
        verifier_incumbent(claim, selected_cost),
        claim,
        limits,
    );
    verify_status(decoded, checked_after, selected_cost, &costs, &mut verifier)?;
    verify_proof_work(decoded, &verifier)?;
    Ok(CheckedCoverage {
        after: checked_after,
        selected_cost,
    })
}

fn verifier_incumbent(claim: &Claim, selected_cost: u64) -> Option<u64> {
    match claim.status {
        VerifiedCoverageStatus::Optimal => Some(selected_cost),
        VerifiedCoverageStatus::Infeasible => None,
        VerifiedCoverageStatus::SearchIncomplete => claim.upper_bound,
    }
}

fn verify_status(
    decoded: &DecodedCoverage,
    checked_after: Evaluation,
    selected_cost: u64,
    costs: &[u64],
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    match decoded.claim.status {
        VerifiedCoverageStatus::Optimal => {
            verify_optimal(&decoded.claim, checked_after, selected_cost, verifier)
        }
        VerifiedCoverageStatus::Infeasible => {
            verify_infeasible(&decoded.claim, checked_after, verifier)
        }
        VerifiedCoverageStatus::SearchIncomplete => {
            verify_incomplete(decoded, checked_after, selected_cost, costs)
        }
    }
}

fn verify_optimal(
    claim: &Claim,
    checked_after: Evaluation,
    selected_cost: u64,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !checked_after.criterion_holds
        || claim.lower_bound != Some(selected_cost)
        || claim.upper_bound != Some(selected_cost)
    {
        return Err(ProofError::new(
            "optimal coverage result has an invalid incumbent or bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("optimal coverage result has no proof tree"))?;
    verifier.verify_root(proof)
}

fn verify_infeasible(
    claim: &Claim,
    checked_after: Evaluation,
    verifier: &mut TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if !claim.selected.is_empty()
        || claim.lower_bound.is_some()
        || claim.upper_bound.is_some()
        || checked_after.criterion_holds
    {
        return Err(ProofError::new(
            "infeasible coverage result has an incumbent or finite bound",
        ));
    }
    let proof = claim
        .proof
        .as_ref()
        .ok_or_else(|| ProofError::new("infeasible coverage result has no proof tree"))?;
    verifier.verify_root(proof)
}

fn verify_incomplete(
    decoded: &DecodedCoverage,
    checked_after: Evaluation,
    selected_cost: u64,
    costs: &[u64],
) -> Result<(), ProofError> {
    let claim = &decoded.claim;
    let root_bound = blocker_bound(costs, &claim.root_blockers)?;
    if claim.proof.is_some()
        || decoded.proof_nodes != 0
        || decoded.proof_topology_checks != 0
        || decoded.proof_terms != 0
        || claim.upper_bound.is_some() != checked_after.criterion_holds
        || claim.upper_bound.is_some_and(|cost| cost != selected_cost)
        || claim.lower_bound != Some(root_bound)
        || claim
            .lower_bound
            .zip(claim.upper_bound)
            .is_some_and(|(lower, upper)| lower > upper)
    {
        return Err(ProofError::new(
            "incomplete coverage result has an invalid gap",
        ));
    }
    Ok(())
}

fn verify_proof_work(
    decoded: &DecodedCoverage,
    verifier: &TreeVerifier<'_>,
) -> Result<(), ProofError> {
    if verifier.nodes != decoded.proof_nodes
        || verifier.checks != decoded.proof_topology_checks
        || verifier.terms != decoded.proof_terms
    {
        return Err(ProofError::new(
            "coverage proof work differs from the checked tree",
        ));
    }
    Ok(())
}

fn coverage_summary(decoded: &DecodedCoverage, checked: CheckedCoverage) -> VerifiedCoverage {
    let claim = &decoded.claim;
    VerifiedCoverage {
        modulus: claim.modulus,
        source: match claim.source {
            Source::Finite => VerifiedCoverageSource::Finite,
            Source::Affine { .. } => VerifiedCoverageSource::Affine,
        },
        states: claim.states.len(),
        actions: claim.actions.len(),
        failure_budget: claim.failure_budget,
        status: claim.status,
        selected: claim.selected.len(),
        total_cost: checked
            .after
            .criterion_holds
            .then_some(checked.selected_cost),
        lower_bound_cost: claim.lower_bound,
        upper_bound_cost: claim.upper_bound,
        producer_oracle_calls: decoded.producer_oracle_calls,
        producer_search_nodes: decoded.producer_search_nodes,
        proof_nodes: decoded.proof_nodes,
        proof_topology_checks: decoded.proof_topology_checks,
        selected_failure_checks: checked.after.checks,
        minimum_witness_triangles: checked.after.minimum_witness,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Edge {
    u: usize,
    v: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct State {
    scenario: u64,
    step: u64,
    base: Vec<usize>,
    edges: Vec<Edge>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Action {
    vertex: usize,
    cost: u64,
    states: Vec<usize>,
}

#[derive(Debug, Clone)]
struct AffineEdge {
    edge: Edge,
    intercept: f64,
    velocity: f64,
}

enum Source {
    Finite,
    Affine {
        scenario: u64,
        edges: Vec<AffineEdge>,
        start: f64,
        end: f64,
    },
}

struct Claim {
    vertex_count: usize,
    broadcast_radius: f64,
    sensing_radius: f64,
    modulus: u32,
    fence: Vec<usize>,
    failable: Vec<usize>,
    failure_budget: usize,
    source: Source,
    states: Vec<State>,
    actions: Vec<Action>,
    max_activations: usize,
    status: VerifiedCoverageStatus,
    selected: Vec<usize>,
    lower_bound: Option<u64>,
    upper_bound: Option<u64>,
    root_blockers: Vec<Vec<usize>>,
    before: Evaluation,
    after: Evaluation,
    proof: Option<ProofNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Evaluation {
    criterion_holds: bool,
    checks: usize,
    minimum_witness: Option<usize>,
}

fn decode_source(reader: &mut Reader<'_>, limits: ProofLimits) -> Result<Source, ProofError> {
    match reader.u8()? {
        0 => Ok(Source::Finite),
        1 => decode_affine_source(reader, limits),
        _ => Err(ProofError::new("coverage source kind is invalid")),
    }
}

fn decode_affine_source(
    reader: &mut Reader<'_>,
    limits: ProofLimits,
) -> Result<Source, ProofError> {
    let scenario = reader.u64()?;
    let start = f64::from_bits(reader.u64()?);
    let end = f64::from_bits(reader.u64()?);
    let count = reader.bounded_usize("affine edge count", limits.max_edges)?;
    if count > reader.remaining() / 32 {
        return Err(ProofError::new(
            "coverage affine edges exceed the remaining bytes",
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
    } = &claim.source
    else {
        return Ok(());
    };
    validate_affine(claim.vertex_count, edges, *start, *end)?;
    let base = claim
        .states
        .first()
        .map(|state| state.base.clone())
        .ok_or_else(|| ProofError::new("coverage affine source has no states"))?;
    if claim.states.iter().any(|state| state.base != base) {
        return Err(ProofError::new(
            "coverage affine source changes its base sensor set",
        ));
    }
    let graphs = complete_threshold_graphs(edges, *start, *end, claim.broadcast_radius, limits)?;
    let expected = graphs
        .into_iter()
        .enumerate()
        .map(|(step, edges)| State {
            scenario: *scenario,
            step: step as u64,
            base: base.clone(),
            edges,
        })
        .collect::<Vec<_>>();
    if expected != claim.states {
        return Err(ProofError::new(
            "coverage states are not the complete affine threshold schedule",
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
        Err(ProofError::new("coverage affine interval is invalid"))
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
            "coverage affine edge trajectory is not canonical",
        ));
    }
    validate_trajectory_weight(trajectory, start)?;
    validate_trajectory_weight(trajectory, end)
}

fn validate_trajectory_weight(trajectory: &AffineEdge, time: f64) -> Result<(), ProofError> {
    let weight = trajectory.intercept + trajectory.velocity * time;
    if !weight.is_finite() || weight < 0.0 {
        Err(ProofError::new(
            "coverage affine edge weight leaves its valid range",
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
        .ok_or_else(|| ProofError::new("coverage affine state count overflows"))?;
    if graph_count > limits.max_snapshots.min(FORMAT_MAX_STATES) {
        return Err(ProofError::new(
            "coverage affine schedule exceeds its state limit",
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

fn evaluate(
    claim: &Claim,
    selected: &[usize],
    limits: ProofLimits,
) -> Result<Evaluation, ProofError> {
    let mut checks = 0usize;
    let mut minimum_witness = None;
    for (state_index, state) in claim.states.iter().enumerate() {
        let mut active = state.base.clone();
        for action in selected {
            if claim.actions[*action]
                .states
                .binary_search(&state_index)
                .is_ok()
            {
                insert_sorted(&mut active, claim.actions[*action].vertex);
            }
        }
        let failable = active
            .iter()
            .copied()
            .filter(|vertex| claim.failable.binary_search(vertex).is_ok())
            .collect::<Vec<_>>();
        let failure_count = claim.failure_budget.min(failable.len());
        let mut combination = Vec::with_capacity(failure_count);
        let mut failure = false;
        visit_combinations(
            &failable,
            failure_count,
            0,
            &mut combination,
            &mut |removed| {
                checks = checks
                    .checked_add(1)
                    .ok_or_else(|| ProofError::new("coverage failure check count overflows"))?;
                if checks > limits.max_snapshots {
                    return Err(ProofError::new(
                        "coverage failure checks exceed their limit",
                    ));
                }
                let remaining = active
                    .iter()
                    .copied()
                    .filter(|vertex| removed.binary_search(vertex).is_err())
                    .collect::<Vec<_>>();
                match relative_criterion(claim, state, &remaining, limits)? {
                    Some(support) => {
                        minimum_witness = Some(minimum_witness.unwrap_or(usize::MAX).min(support));
                        Ok(true)
                    }
                    None => {
                        failure = true;
                        Ok(false)
                    }
                }
            },
        )?;
        if failure {
            return Ok(Evaluation {
                criterion_holds: false,
                checks,
                minimum_witness: None,
            });
        }
    }
    Ok(Evaluation {
        criterion_holds: true,
        checks,
        minimum_witness,
    })
}

fn relative_criterion(
    claim: &Claim,
    state: &State,
    active: &[usize],
    limits: ProofLimits,
) -> Result<Option<usize>, ProofError> {
    let active_set = active.iter().copied().collect::<BTreeSet<_>>();
    let edges = state
        .edges
        .iter()
        .copied()
        .filter(|edge| active_set.contains(&edge.u) && active_set.contains(&edge.v))
        .collect::<Vec<_>>();
    for (u, v) in cycle_pairs(&claim.fence) {
        if edges.binary_search(&edge(u, v)).is_err() {
            return Err(ProofError::new(
                "coverage graph omits a consecutive fence edge",
            ));
        }
    }
    let triangles = flag_triangles(active, &edges, limits.max_triangles)?;
    let entries = edges
        .len()
        .checked_mul(triangles.len().saturating_add(1))
        .ok_or_else(|| ProofError::new("coverage linear-system size overflows"))?;
    if entries > limits.max_terms {
        return Err(ProofError::new(
            "coverage linear system exceeds its entry limit",
        ));
    }
    let target = fence_chain(&claim.fence, &edges, claim.modulus)?;
    Ok(solve_boundary(&edges, &triangles, &target, claim.modulus)
        .map(|solution| solution.into_iter().filter(|value| *value != 0).count()))
}

fn flag_triangles(
    vertices: &[usize],
    edges: &[Edge],
    maximum: usize,
) -> Result<Vec<[usize; 3]>, ProofError> {
    let edge_set = edges.iter().copied().collect::<BTreeSet<_>>();
    let mut triangles = Vec::new();
    for (first_position, &a) in vertices.iter().enumerate() {
        for (second_position, &b) in vertices.iter().enumerate().skip(first_position + 1) {
            if !edge_set.contains(&edge(a, b)) {
                continue;
            }
            for &c in vertices.iter().skip(second_position + 1) {
                if edge_set.contains(&edge(a, c)) && edge_set.contains(&edge(b, c)) {
                    triangles.push([a, b, c]);
                    if triangles.len() > maximum {
                        return Err(ProofError::new(
                            "coverage active triangles exceed their limit",
                        ));
                    }
                }
            }
        }
    }
    Ok(triangles)
}

fn fence_chain(fence: &[usize], edges: &[Edge], modulus: u32) -> Result<Vec<u32>, ProofError> {
    let positions = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect::<BTreeMap<_, _>>();
    let mut chain = vec![0u32; edges.len()];
    for (u, v) in cycle_pairs(fence) {
        let position = positions[&edge(u, v)];
        let coefficient = if u < v { 1 } else { modulus - 1 };
        chain[position] = add(chain[position], coefficient, modulus);
    }
    if chain.iter().all(|value| *value == 0) {
        return Err(ProofError::new(
            "coverage fence cycle is zero in the declared field",
        ));
    }
    Ok(chain)
}

fn solve_boundary(
    edges: &[Edge],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Option<Vec<u32>> {
    let mut rows = boundary_rows(edges, triangles, target, modulus);
    let pivots = reduce_boundary_rows(&mut rows, triangles.len(), modulus);
    if boundary_inconsistent(&rows, triangles.len()) {
        return None;
    }
    Some(boundary_solution(&rows, &pivots, triangles.len()))
}

fn boundary_rows(
    edges: &[Edge],
    triangles: &[[usize; 3]],
    target: &[u32],
    modulus: u32,
) -> Vec<Vec<u32>> {
    let positions = edges
        .iter()
        .copied()
        .enumerate()
        .map(|(position, edge)| (edge, position))
        .collect::<BTreeMap<_, _>>();
    let mut rows = vec![vec![0u32; triangles.len() + 1]; edges.len()];
    for (column, &[a, b, c]) in triangles.iter().enumerate() {
        rows[positions[&edge(b, c)]][column] = 1;
        rows[positions[&edge(a, c)]][column] = modulus - 1;
        rows[positions[&edge(a, b)]][column] = 1;
    }
    for (row, value) in rows.iter_mut().zip(target) {
        row[triangles.len()] = *value;
    }
    rows
}

fn reduce_boundary_rows(rows: &mut [Vec<u32>], columns: usize, modulus: u32) -> Vec<usize> {
    let mut pivot_row = 0usize;
    let mut pivots = Vec::new();
    for column in 0..columns {
        let Some(found) = (pivot_row..rows.len()).find(|row| rows[*row][column] != 0) else {
            continue;
        };
        rows.swap(pivot_row, found);
        normalize_boundary_pivot(rows, pivot_row, column, modulus);
        eliminate_boundary_pivot(rows, pivot_row, column, modulus);
        pivots.push(column);
        pivot_row += 1;
        if pivot_row == rows.len() {
            break;
        }
    }
    pivots
}

fn normalize_boundary_pivot(rows: &mut [Vec<u32>], pivot_row: usize, column: usize, modulus: u32) {
    let inverse = inverse(rows[pivot_row][column], modulus);
    for value in &mut rows[pivot_row][column..] {
        *value = multiply(*value, inverse, modulus);
    }
}

fn eliminate_boundary_pivot(rows: &mut [Vec<u32>], pivot_row: usize, column: usize, modulus: u32) {
    let pivot = rows[pivot_row][column..].to_vec();
    for (row_index, row) in rows.iter_mut().enumerate() {
        if row_index == pivot_row || row[column] == 0 {
            continue;
        }
        let factor = row[column];
        for (value, pivot_value) in row[column..].iter_mut().zip(&pivot) {
            *value = subtract(*value, multiply(factor, *pivot_value, modulus), modulus);
        }
    }
}

fn boundary_inconsistent(rows: &[Vec<u32>], columns: usize) -> bool {
    rows.iter()
        .any(|row| row[..columns].iter().all(|value| *value == 0) && row[columns] != 0)
}

fn boundary_solution(rows: &[Vec<u32>], pivots: &[usize], columns: usize) -> Vec<u32> {
    let mut solution = vec![0u32; columns];
    for (row, &column) in pivots.iter().enumerate() {
        solution[column] = rows[row][columns];
    }
    solution
}

#[derive(Clone, Copy)]
enum BoundKind {
    Cost,
    Activations,
}

enum ProofNode {
    Cost,
    SurvivingMaximum,
    SurvivingActivationLimit,
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
    max_activations: usize,
    cutoff: Option<u64>,
    claim: &'a Claim,
    limits: ProofLimits,
    nodes: usize,
    checks: usize,
    terms: usize,
}

impl<'a> TreeVerifier<'a> {
    fn new(
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
            ProofNode::SurvivingActivationLimit => self.verify_activation_leaf(&included),
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

    fn verify_cost_leaf(&self, included_cost: u64) -> Result<(), ProofError> {
        if self.cutoff.is_none_or(|cutoff| included_cost < cutoff) {
            Err(ProofError::new(
                "coverage cost leaf does not reach the incumbent",
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
                "coverage maximal-survival leaf is feasible",
            ))
        } else {
            Ok(())
        }
    }

    fn verify_activation_leaf(&mut self, included: &[usize]) -> Result<(), ProofError> {
        if included.len() != self.max_activations || !self.check_survival(included)? {
            Err(ProofError::new("coverage activation-limit leaf is invalid"))
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
                "coverage blocker leaf does not close its branch",
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

    fn verify_branch(
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

    fn check_survival(&mut self, selected: &[usize]) -> Result<bool, ProofError> {
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

fn verify_root_blockers(
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
        3 => Ok(ProofNode::SurvivingActivationLimit),
        4 => decode_bound_node(reader, action_count, terms, limits),
        5 => decode_branch_node(reader, action_count, depth, nodes, terms, limits),
        _ => Err(ProofError::new("coverage proof node kind is invalid")),
    }
}

fn record_decoded_node(
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
        2 => Ok(BoundKind::Activations),
        _ => Err(ProofError::new("coverage proof bound kind is invalid")),
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

fn decode_evaluation(reader: &mut Reader<'_>) -> Result<Evaluation, ProofError> {
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

fn decode_edges(
    reader: &mut Reader<'_>,
    vertex_count: usize,
    maximum: usize,
) -> Result<Vec<Edge>, ProofError> {
    let count = reader.bounded_usize("edge count", maximum)?;
    if count > reader.remaining() / 16 {
        return Err(ProofError::new(
            "coverage edge count exceeds the remaining bytes",
        ));
    }
    let edges = (0..count)
        .map(|_| {
            Ok(Edge {
                u: reader.usize()?,
                v: reader.usize()?,
            })
        })
        .collect::<Result<Vec<_>, ProofError>>()?;
    if edges
        .iter()
        .any(|edge| edge.u >= edge.v || edge.v >= vertex_count)
        || edges.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(ProofError::new("coverage edge list is not canonical"));
    }
    Ok(edges)
}

fn decode_usizes(reader: &mut Reader<'_>, maximum: usize) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("integer list count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(
            "coverage integer list exceeds the remaining bytes",
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
        return Err(ProofError::new("coverage index list is not canonical"));
    }
    Ok(values)
}

fn visit_combinations<F>(
    values: &[usize],
    count: usize,
    start: usize,
    current: &mut Vec<usize>,
    callback: &mut F,
) -> Result<bool, ProofError>
where
    F: FnMut(&[usize]) -> Result<bool, ProofError>,
{
    if current.len() == count {
        return callback(current);
    }
    let needed = count - current.len();
    for position in start..=values.len() - needed {
        current.push(values[position]);
        if !visit_combinations(values, count, position + 1, current, callback)? {
            current.pop();
            return Ok(false);
        }
        current.pop();
    }
    Ok(true)
}

fn validate_radii(broadcast: f64, sensing: f64) -> Result<(), ProofError> {
    if !broadcast.is_finite() || !sensing.is_finite() || broadcast <= 0.0 || sensing <= 0.0 {
        return Err(ProofError::new(
            "coverage radii must be finite and positive",
        ));
    }
    let broadcast = rational(broadcast);
    let sensing = rational(sensing);
    if BigRational::from_integer(3.into()) * &sensing * sensing < broadcast.clone() * broadcast {
        return Err(ProofError::new(
            "coverage radii violate the exact controlled-boundary inequality",
        ));
    }
    Ok(())
}

fn canonical_fence(vertices: &[usize]) -> Vec<usize> {
    let forward = rotate_to_minimum(vertices);
    let mut reversed = vertices.to_vec();
    reversed.reverse();
    let reversed = rotate_to_minimum(&reversed);
    forward.min(reversed)
}

fn rotate_to_minimum(values: &[usize]) -> Vec<usize> {
    let position = values
        .iter()
        .enumerate()
        .min_by_key(|(_, value)| **value)
        .map(|(position, _)| position)
        .unwrap_or(0);
    values[position..]
        .iter()
        .chain(&values[..position])
        .copied()
        .collect()
}

fn cycle_pairs(vertices: &[usize]) -> impl Iterator<Item = (usize, usize)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
}

fn edge(u: usize, v: usize) -> Edge {
    Edge {
        u: u.min(v),
        v: u.max(v),
    }
}

fn add(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(right)) % u64::from(modulus)) as u32
}

fn subtract(left: u32, right: u32, modulus: u32) -> u32 {
    ((u64::from(left) + u64::from(modulus) - u64::from(right)) % u64::from(modulus)) as u32
}

fn multiply(left: u32, right: u32, modulus: u32) -> u32 {
    (u64::from(left) * u64::from(right) % u64::from(modulus)) as u32
}

fn inverse(value: u32, modulus: u32) -> u32 {
    let mut result = 1u64;
    let mut base = u64::from(value);
    let mut exponent = u64::from(modulus - 2);
    let modulus = u64::from(modulus);
    while exponent != 0 {
        if exponent & 1 == 1 {
            result = result * base % modulus;
        }
        base = base * base % modulus;
        exponent >>= 1;
    }
    result as u32
}

fn rational(value: f64) -> BigRational {
    BigRational::from_float(value).expect("validated finite f64 has an exact rational form")
}

fn midpoint(left: &BigRational, right: &BigRational) -> BigRational {
    (left + right) / BigRational::from_integer(2.into())
}

fn selected_cost(costs: &[u64], selected: &[usize]) -> Result<u64, ProofError> {
    selected.iter().try_fold(0u64, |sum, candidate| {
        sum.checked_add(costs[*candidate])
            .ok_or_else(|| ProofError::new("coverage selected cost overflows"))
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
            .ok_or_else(|| ProofError::new("coverage blocker bound overflows"))
    })
}

fn add_terms(total: &mut usize, count: usize, limits: ProofLimits) -> Result<(), ProofError> {
    *total = total
        .checked_add(count)
        .ok_or_else(|| ProofError::new("coverage proof term count overflows"))?;
    if *total > limits.max_terms.min(FORMAT_MAX_PROOF_TERMS) {
        return Err(ProofError::new("coverage proof terms exceed their limit"));
    }
    Ok(())
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
            .ok_or_else(|| ProofError::new("coverage artifact position overflows"))?;
        if end > self.bytes.len() {
            return Err(ProofError::new("coverage artifact is truncated"));
        }
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn bounded_usize(&mut self, name: &str, maximum: usize) -> Result<usize, ProofError> {
        let value = self.usize()?;
        if value > maximum {
            return Err(ProofError::new(format!(
                "coverage {name} exceeds its limit"
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
            .map_err(|_| ProofError::new("coverage integer does not fit usize"))
    }

    fn optional_u64(&mut self) -> Result<Option<u64>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.u64()?)),
            _ => Err(ProofError::new("coverage optional integer flag is invalid")),
        }
    }

    fn optional_usize(&mut self) -> Result<Option<usize>, ProofError> {
        match self.u8()? {
            0 => Ok(None),
            1 => Ok(Some(self.usize()?)),
            _ => Err(ProofError::new("coverage optional integer flag is invalid")),
        }
    }

    fn array32(&mut self) -> Result<[u8; 32], ProofError> {
        Ok(self.take(32)?.try_into().unwrap())
    }
}
