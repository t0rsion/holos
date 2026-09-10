use std::collections::BTreeSet;

use sha2::{Digest, Sha256};

use crate::{ProofError, ProofLimits};

use super::evaluate::add_terms;
use super::model::*;
use super::proof::{decode_evaluation, decode_proof};
use super::wire_header::{decode_header, decode_prefix};
pub(crate) use super::wire_reader::Reader;
use super::{
    FORMAT_MAX_ACTIONS, FORMAT_MAX_ORACLE_CALLS, FORMAT_MAX_SEARCH_NODES, FORMAT_MAX_STATES,
};

pub(crate) fn decode_coverage(
    bytes: &[u8],
    limits: ProofLimits,
) -> Result<DecodedCoverage, ProofError> {
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

pub(crate) fn expected_digest(bytes: &[u8], limits: ProofLimits) -> Result<[u8; 32], ProofError> {
    if bytes.len() > limits.max_bytes || bytes.len() < 32 {
        return Err(ProofError::new(
            "coverage artifact exceeds its byte limit or is truncated",
        ));
    }
    Ok(Sha256::digest(&bytes[..bytes.len() - 32]).into())
}

pub(crate) fn decode_states(
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

pub(crate) fn decode_state(
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

pub(crate) fn validate_state_order(
    prior: Option<(u64, u64)>,
    state: &State,
) -> Result<(), ProofError> {
    if prior.is_some_and(|key| key >= (state.scenario, state.step)) {
        return Err(ProofError::new(
            "coverage states are not in canonical scenario and step order",
        ));
    }
    Ok(())
}

pub(crate) fn add_edge_count(
    total: usize,
    add: usize,
    limits: ProofLimits,
) -> Result<usize, ProofError> {
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

pub(crate) fn decode_actions(
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

pub(crate) fn decode_action(
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

pub(crate) fn validate_action(
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

pub(crate) fn decode_search(
    reader: &mut Reader<'_>,
    action_count: usize,
) -> Result<SearchData, ProofError> {
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

pub(crate) fn decode_work_limits(reader: &mut Reader<'_>) -> Result<WorkLimits, ProofError> {
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

pub(crate) fn decode_selection(
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

pub(crate) fn decode_proof_data(
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

pub(crate) fn decode_root_blockers(
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

pub(crate) fn decode_optional_proof(
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

pub(crate) fn decode_trailer(
    reader: &mut Reader<'_>,
    expected: [u8; 32],
) -> Result<(), ProofError> {
    if reader.array32()? != expected || reader.remaining() != 0 {
        return Err(ProofError::new(
            "coverage artifact has a wrong digest or trailing bytes",
        ));
    }
    Ok(())
}

pub(crate) fn decode_edges(
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

pub(crate) fn decode_usizes(
    reader: &mut Reader<'_>,
    maximum: usize,
) -> Result<Vec<usize>, ProofError> {
    let count = reader.bounded_usize("integer list count", maximum)?;
    if count > reader.remaining() / 8 {
        return Err(ProofError::new(
            "coverage integer list exceeds the remaining bytes",
        ));
    }
    (0..count).map(|_| reader.usize()).collect()
}

pub(crate) fn decode_indices(
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

pub(crate) fn visit_combinations<F>(
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
